//! Transcription worker (Phase 3 wiring).
//!
//! Runs the [`Processor`](super::processor::Processor) pipeline on a dedicated
//! thread so the (CPU-heavy, possibly long) whisper + diarization work never
//! blocks Tauri's IPC thread. Progress is surfaced two ways:
//!
//! 1. **Polled** — written into [`AppState::transcription`], which the existing
//!    `get_transcription_status` command reads (the frontend polls it).
//! 2. **Pushed** — emitted as Tauri events (`transcription://progress` /
//!    `://done` / `://error`).
//!
//! With the `whisper` feature the model is downloaded on first use (into the
//! app-data `models/` dir) and the real pipeline runs. Without it,
//! [`Processor::transcribe_wav`](super::processor::Processor::transcribe_wav)
//! returns `FeatureDisabled`, which the worker treats as "recording ok, no
//! transcript" (meeting → Completed) rather than an error.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::ai::keychain;
use crate::commands::{AppState, TranscriptionProgressEntry};
use crate::models::{AiProviderKind, MeetingStatus};

use super::model::{default_model, lookup, ModelManager};
use super::processor::{Processor, ProcessorOptions};
use super::whisper::WhisperOptions;
use super::{Progress, ProgressStage};

/// Event payload pushed to the frontend on each progress tick / terminal state.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    meeting_id: String,
    stage: ProgressStage,
    fraction: f32,
    message: String,
}

/// Coarse stage string stored for the status poll (the DTO's `stage` is free
/// text; this keeps it readable).
fn stage_str(stage: ProgressStage) -> &'static str {
    match stage {
        ProgressStage::PreparingModel => "preparing_model",
        ProgressStage::LoadingAudio => "loading_audio",
        ProgressStage::Transcribing => "transcribing",
        ProgressStage::Diarizing => "diarizing",
        ProgressStage::Done => "done",
    }
}

/// Record a progress tick: update the polled map + emit a `progress` event.
fn report(
    app: &AppHandle,
    state: &State<'_, AppState>,
    meeting_id: &str,
    stage: ProgressStage,
    fraction: f32,
    message: String,
) {
    if let Ok(mut map) = state.transcription.lock() {
        map.insert(
            meeting_id.to_string(),
            TranscriptionProgressEntry {
                stage: stage_str(stage).to_string(),
                fraction,
                message: Some(message.clone()),
            },
        );
    }
    let _ = app.emit(
        "transcription://progress",
        ProgressEvent {
            meeting_id: meeting_id.to_string(),
            stage,
            fraction,
            message,
        },
    );
}

/// Spawn the transcription pipeline for `meeting_id` over `wav_path`. Returns
/// immediately; progress + completion are reported via the app state map and
/// Tauri events. `model_id` selects the whisper model (falls back to `small` if
/// unknown); `diarize` toggles speaker labelling.
/// Per-meeting transcription request: which engine + the engine-specific config.
pub struct TranscriptionRequest {
    /// Local whisper model id (used when the whisper engine runs).
    pub model_id: String,
    pub diarize: bool,
    /// Forced language ("zh"/"en"/…) or `None` for auto-detect.
    pub language: Option<String>,
    /// `"auto"` (Gemini if an API key is set, else whisper) | `"gemini"` | `"whisper"`.
    pub engine: String,
    /// Multimodal model id for the Gemini engine.
    pub gemini_model: String,
}

pub fn spawn_transcription(
    app: AppHandle,
    meeting_id: String,
    wav_path: PathBuf,
    req: TranscriptionRequest,
) {
    let _ = std::thread::Builder::new()
        .name("transcription".into())
        .spawn(move || run(app, meeting_id, wav_path, req));
}

/// Dispatch to the Gemini multimodal engine (primary, when a key is present) or
/// the local whisper engine. On a Gemini failure with the `whisper` feature
/// built, fall through to whisper as a backup.
fn run(app: AppHandle, meeting_id: String, wav_path: PathBuf, req: TranscriptionRequest) {
    let has_key = keychain::get_api_key(AiProviderKind::Gemini)
        .ok()
        .flatten()
        .is_some();
    let use_gemini = match req.engine.as_str() {
        "gemini" => true,
        "whisper" => false,
        _ => has_key, // "auto"
    };

    if use_gemini {
        let state = app.state::<AppState>();
        if !has_key {
            finish_error(
                &app,
                &state,
                &meeting_id,
                "已選 Gemini 引擎,但尚未設定 Gemini API key(Settings → AI)。".into(),
            );
            return;
        }
        match try_gemini(&app, &state, &meeting_id, &wav_path, &req) {
            Ok(()) => return,
            Err(msg) => {
                #[cfg(feature = "whisper")]
                report(
                    &app,
                    &state,
                    &meeting_id,
                    ProgressStage::PreparingModel,
                    0.0,
                    format!("Gemini 失敗,改用本地 whisper:{msg}"),
                );
                #[cfg(not(feature = "whisper"))]
                {
                    finish_error(
                        &app,
                        &state,
                        &meeting_id,
                        format!("Gemini transcription failed: {msg}"),
                    );
                    return;
                }
            }
        }
        // `state` (borrowing `app`) drops here, before `app` moves into run_whisper.
    }

    run_whisper(app, meeting_id, wav_path, req.model_id, req.diarize, req.language);
}

/// Overlay real speaker names onto freshly transcribed segments using the
/// recording's `speakers.srt` identity sidecar (spec 2026-07-03), by mechanical
/// overlap-join. The sidecar sits next to the WAV; when it is absent (non-Teams,
/// non-Windows, or the UIA capture read nothing) this is a silent no-op, so it is
/// safe to call unconditionally on both the Gemini and whisper paths.
fn apply_speaker_identity(wav_path: &PathBuf, segments: &mut [crate::models::TranscriptSegment]) {
    let srt_path = wav_path.with_file_name("speakers.srt");
    let Ok(text) = std::fs::read_to_string(&srt_path) else {
        return;
    };
    let spans = crate::detection::speaker::parse_speaker_srt(&text);
    if spans.is_empty() {
        return;
    }
    crate::detection::speaker::assign_speakers(
        segments,
        &spans,
        crate::detection::speaker::DEFAULT_MIN_OVERLAP_FRAC,
    );
}

/// Run local diarization over the Gemini transcript's own audio and stamp each
/// segment with its acoustic speaker ("Speaker N"), overriding Gemini's guess.
/// Diarization models are bundled with the app (tauri resource dir). Best-effort:
/// any failure (missing models, decode error) leaves Gemini's labels untouched.
/// Compiled in only with the `diarize` feature (sherpa-onnx native); without it
/// the caller skips this entirely.
#[cfg(feature = "diarize")]
fn diarize_gemini_segments(
    app: &AppHandle,
    state: &State<'_, AppState>,
    meeting_id: &str,
    wav_path: &PathBuf,
    segments: &mut [crate::models::TranscriptSegment],
) {
    use crate::transcription::diarization as d;
    use tauri::Manager;
    let Ok(pcm) = crate::transcription::processor::Processor::load_wav_16k_mono(wav_path) else {
        return;
    };
    let Ok(res_dir) = app.path().resource_dir() else {
        return;
    };
    let model_dir = res_dir.join("models");
    let diarizer = d::Diarizer::new(d::DiarizeConfig {
        segmentation_model: model_dir.join("sherpa-segmentation.onnx"),
        embedding_model: model_dir.join("sherpa-embedding.onnx"),
        num_speakers: None,
        cluster_threshold: d::DEFAULT_CLUSTER_THRESHOLD,
    });
    let Ok(turns) = diarizer.diarize(&pcm) else {
        return;
    };
    d::apply_turns_to_segments(segments, &turns);
    if !turns.is_empty() {
        // Phase 3: embed each acoustic cluster, stage it, and auto-name recurring
        // speakers from the enrolled voiceprint library. Best-effort.
        voiceprint_clusters(state, meeting_id, &model_dir, &pcm, &turns);
    }
}

/// Compute a voiceprint per diarization cluster, stage it for later enrollment,
/// and prefill any cluster matching an enrolled speaker with their name
/// (`source = 'voiceprint'`). Embedding extraction is slow, so it runs WITHOUT
/// holding the db lock; persistence happens under one short lock afterwards.
#[cfg(feature = "diarize")]
fn voiceprint_clusters(
    state: &State<'_, AppState>,
    meeting_id: &str,
    model_dir: &std::path::Path,
    pcm: &[f32],
    turns: &[crate::transcription::diarization::SpeakerTurn],
) {
    use crate::transcription::diarization as d;
    use sherpa_rs::speaker_id::{EmbeddingExtractor, ExtractorConfig};

    // Distinct cluster ids, ascending.
    let mut ids: Vec<u32> = turns.iter().map(|t| t.speaker_id).collect();
    ids.sort_unstable();
    ids.dedup();

    let Ok(mut extractor) = EmbeddingExtractor::new(ExtractorConfig {
        model: model_dir
            .join("sherpa-embedding.onnx")
            .to_string_lossy()
            .into_owned(),
        provider: None,
        num_threads: None,
        debug: false,
    }) else {
        return;
    };

    // Enrolled library, decoded once, for matching.
    let enrolled = match state.db.lock() {
        Ok(db) => db.enrolled_voiceprints().unwrap_or_default(),
        Err(_) => return,
    };

    // (raw_label, embedding, matched_name) — computed lock-free.
    let mut staged: Vec<(String, Vec<f32>, Option<String>)> = Vec::new();
    for id in ids {
        let cluster = d::cluster_pcm_for(turns, pcm, id, 16_000);
        if cluster.is_empty() {
            continue;
        }
        let Ok(emb) = extractor.compute_speaker_embedding(cluster, 16_000) else {
            continue;
        };
        let matched = d::best_voiceprint_match(&emb, &enrolled, d::VOICEPRINT_MATCH_THRESHOLD);
        staged.push((d::speaker_label(id), emb, matched));
    }

    if let Ok(db) = state.db.lock() {
        for (raw_label, emb, matched) in staged {
            let _ = db.store_cluster_embedding(meeting_id, &raw_label, &emb);
            if let Some(name) = matched {
                let _ = db.prefill_speaker_label_from_voiceprint(meeting_id, &raw_label, &name);
            }
        }
    }
}

/// Gemini multimodal engine: upload the WAV, get transcript + summary in one
/// call, persist both, mark the meeting Completed. Returns `Err(msg)` on any
/// failure so the caller can fall back to whisper.
fn try_gemini(
    app: &AppHandle,
    state: &State<'_, AppState>,
    meeting_id: &str,
    wav_path: &PathBuf,
    req: &TranscriptionRequest,
) -> std::result::Result<(), String> {
    report(
        app,
        state,
        meeting_id,
        ProgressStage::PreparingModel,
        0.05,
        "上傳音訊到 Gemini…".into(),
    );

    // Summary template follows the meeting type (same as generate_summary).
    let template = {
        let db = state.db.lock().map_err(|_| "database lock poisoned".to_string())?;
        let mt = db
            .get_meeting(meeting_id)
            .ok()
            .flatten()
            .and_then(|m| m.meeting_type);
        crate::ai::SummaryTemplate::for_meeting_type(mt)
    };
    let created_at = crate::commands::now_iso8601();

    report(
        app,
        state,
        meeting_id,
        ProgressStage::Transcribing,
        0.3,
        "Gemini 轉錄 + 摘要中…".into(),
    );

    // The worker is a std::thread with no ambient runtime — like download_model,
    // spin a throwaway current-thread runtime for the async upload + request.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let mut res = rt
        .block_on(crate::ai::gemini_audio::transcribe_and_summarize(
            wav_path,
            meeting_id,
            &created_at,
            template,
            &req.gemini_model,
            // Per-chunk progress for long recordings (otherwise the UI sits at
            // one percentage for minutes).
            |fraction: f32, msg: &str| {
                report(app, state, meeting_id, ProgressStage::Transcribing, fraction, msg.to_string());
            },
        ))
        .map_err(|e| e.to_string())?;

    // Diarization owns segmentation: override Gemini's speaker guesses with the
    // real acoustic "Speaker N" clusters where diarization covers a segment.
    // No-op without the `diarize` feature (empty turns → Gemini's own label
    // survives, no regression). Runs BEFORE the identity sidecar so real names
    // can overlay on top of the baseline speaker.
    #[cfg(feature = "diarize")]
    diarize_gemini_segments(app, state, meeting_id, wav_path, &mut res.segments);

    // Overlay real speaker names from the recording's identity sidecar, if any.
    apply_speaker_identity(wav_path, &mut res.segments);

    let n = res.segments.len();
    if let Ok(db) = state.db.lock() {
        let run = crate::models::TranscriptRun {
            id: uuid::Uuid::new_v4().to_string(),
            meeting_id: meeting_id.to_string(),
            engine: "gemini".to_string(),
            model: req.gemini_model.clone(),
            language: req.language.clone(),
            created_at: String::new(),
            segment_count: 0,
        };
        let _ = db.insert_transcript_run(&run);
        let _ = db.insert_transcript_segments(&res.segments, Some(&run.id));
        let summary = crate::models::Summary {
            id: uuid::Uuid::new_v4().to_string(),
            meeting_id: meeting_id.to_string(),
            summary_type: crate::models::SummaryType::Auto,
            content: res.summary.content,
            action_items: res.summary.action_items,
            key_decisions: res.summary.key_decisions,
            prompt_used: None,
            ai_provider: Some(res.summary.provider),
            ai_model: Some(res.summary.model),
            tokens_used: res.summary.tokens_used,
            created_at: String::new(),
        };
        let _ = db.insert_summary(&summary);
        let _ = db.set_meeting_status(meeting_id, MeetingStatus::Completed);
        // Write transcript.md / summary.md next to the audio.
        crate::commands::write_meeting_sidecars(&db, &state.files, meeting_id);
    }

    let msg = format!("{n} segments + summary (Gemini)");
    if let Ok(mut map) = state.transcription.lock() {
        map.insert(
            meeting_id.to_string(),
            TranscriptionProgressEntry {
                stage: "done".to_string(),
                fraction: 1.0,
                message: Some(msg.clone()),
            },
        );
    }
    let _ = app.emit(
        "transcription://done",
        ProgressEvent {
            meeting_id: meeting_id.to_string(),
            stage: ProgressStage::Done,
            fraction: 1.0,
            message: msg,
        },
    );
    Ok(())
}

/// Run the local whisper pipeline (the original transcription path).
fn run_whisper(
    app: AppHandle,
    meeting_id: String,
    wav_path: PathBuf,
    model_id: String,
    diarize: bool,
    language: Option<String>,
) {
    let cache_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("models");
    let manager = ModelManager::new(cache_dir);
    let model = lookup(&model_id).unwrap_or_else(|_| default_model());
    let state = app.state::<AppState>();

    // The local whisper pipeline only reads 16 kHz mono WAV (recordings are
    // already that). Imported non-wav files must go through Gemini.
    let is_wav = wav_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    if !is_wav {
        finish_error(
            &app,
            &state,
            &meeting_id,
            "本地 whisper 僅支援 wav 檔;此匯入檔請改用 Gemini 引擎(Settings → 轉錄引擎)。".into(),
        );
        return;
    }

    // Ensure the whisper model is on disk (download on first use). Only compiled
    // in with the `whisper` feature; without it `transcribe_wav` returns
    // FeatureDisabled below and no download is attempted.
    #[cfg(feature = "whisper")]
    if !manager.is_cached(model) {
        report(
            &app,
            &state,
            &meeting_id,
            ProgressStage::PreparingModel,
            0.0,
            format!(
                "downloading {} model (~{} MB) — first run only…",
                model.id, model.approx_size_mb
            ),
        );
        if let Err(e) = download_model(&manager, model, &app, &state, &meeting_id) {
            finish_error(&app, &state, &meeting_id, format!("model download failed: {e}"));
            return;
        }
    }

    let processor = Processor::new(manager);
    let created_at = crate::commands::now_iso8601();
    // Convert Simplified→Traditional afterwards for Chinese / auto-detect (None);
    // skip for explicit en/ja. Computed before `language` is moved into options.
    #[cfg_attr(not(feature = "opencc"), allow(unused_variables))]
    let want_traditional = language.as_deref().map_or(true, |l| l.starts_with("zh"));
    // Keep a copy for the run row before `language` moves into the options.
    let run_language = language.clone();
    let options = ProcessorOptions {
        whisper: WhisperOptions {
            language, // default "zh" (set in commands::transcription_settings)
            ..Default::default()
        },
        diarize,
    };

    let result = processor.transcribe_wav(
        &wav_path,
        model,
        &meeting_id,
        &created_at,
        &options,
        |p: Progress| report(&app, &state, &meeting_id, p.stage, p.fraction, p.message),
    );

    match result {
        Ok(segments) => {
            // `mut` is always used now: the identity overlay below takes
            // `&mut segments` on every build, so no cfg gate is needed.
            let mut segments = segments;
            // Simplified→Traditional (OpenCC s2twp) when transcribing Chinese.
            #[cfg(feature = "opencc")]
            if want_traditional {
                super::zhconvert::segments_to_traditional(&mut segments);
            }
            // Overlay real speaker names from the recording's identity sidecar.
            apply_speaker_identity(&wav_path, &mut segments);
            if let Ok(db) = state.db.lock() {
                let run = crate::models::TranscriptRun {
                    id: uuid::Uuid::new_v4().to_string(),
                    meeting_id: meeting_id.clone(),
                    engine: "whisper".to_string(),
                    model: model_id.clone(),
                    language: run_language.clone(),
                    created_at: String::new(),
                    segment_count: 0,
                };
                let _ = db.insert_transcript_run(&run);
                let _ = db.insert_transcript_segments(&segments, Some(&run.id));
                let _ = db.set_meeting_status(&meeting_id, MeetingStatus::Completed);
                crate::commands::write_meeting_sidecars(&db, &state.files, &meeting_id);
            }
            let msg = format!("{} segments", segments.len());
            if let Ok(mut map) = state.transcription.lock() {
                map.insert(
                    meeting_id.clone(),
                    TranscriptionProgressEntry {
                        stage: "done".to_string(),
                        fraction: 1.0,
                        message: Some(msg.clone()),
                    },
                );
            }
            let _ = app.emit(
                "transcription://done",
                ProgressEvent {
                    meeting_id,
                    stage: ProgressStage::Done,
                    fraction: 1.0,
                    message: msg,
                },
            );
        }
        Err(e) => {
            let message = e.to_string();
            // A build without the `whisper` feature is a capability gap, not a
            // failed recording: the audio is fine, there's just no transcript.
            let feature_off = matches!(e, super::TranscriptionError::FeatureDisabled(_));
            let (status, stage) = if feature_off {
                (MeetingStatus::Completed, "done")
            } else {
                (MeetingStatus::Error, "error")
            };
            if let Ok(db) = state.db.lock() {
                let _ = db.set_meeting_status(&meeting_id, status);
            }
            if let Ok(mut map) = state.transcription.lock() {
                map.insert(
                    meeting_id.clone(),
                    TranscriptionProgressEntry {
                        stage: stage.to_string(),
                        fraction: if feature_off { 1.0 } else { 0.0 },
                        message: Some(message.clone()),
                    },
                );
            }
            let _ = app.emit(
                if feature_off {
                    "transcription://done"
                } else {
                    "transcription://error"
                },
                ProgressEvent {
                    meeting_id,
                    stage: ProgressStage::Done,
                    fraction: if feature_off { 1.0 } else { 0.0 },
                    message,
                },
            );
        }
    }
}

/// Download the whisper model on a throwaway current-thread tokio runtime (the
/// worker is a plain `std::thread`, so it has no ambient runtime for reqwest).
#[cfg(feature = "whisper")]
fn download_model(
    manager: &ModelManager,
    model: &super::model::ModelInfo,
    app: &AppHandle,
    state: &State<'_, AppState>,
    meeting_id: &str,
) -> super::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| super::TranscriptionError::Download(e.to_string()))?;
    rt.block_on(manager.ensure_model(model, |p| {
        // Model prep maps to the first 5% of the bar (the processor's
        // PreparingModel weight), so the download doesn't pin the bar at 0.
        let frac = p.fraction().unwrap_or(0.0) * 0.05;
        report(
            app,
            state,
            meeting_id,
            ProgressStage::PreparingModel,
            frac,
            format!("downloading {} model", model.id),
        );
    }))?;
    Ok(())
}

/// Terminal failure: mark the meeting errored + report the message.
fn finish_error(app: &AppHandle, state: &State<'_, AppState>, meeting_id: &str, message: String) {
    eprintln!("[transcription] meeting {meeting_id} failed: {message}");
    if let Ok(db) = state.db.lock() {
        let _ = db.set_meeting_status(meeting_id, MeetingStatus::Error);
    }
    if let Ok(mut map) = state.transcription.lock() {
        map.insert(
            meeting_id.to_string(),
            TranscriptionProgressEntry {
                stage: "error".to_string(),
                fraction: 0.0,
                message: Some(message.clone()),
            },
        );
    }
    let _ = app.emit(
        "transcription://error",
        ProgressEvent {
            meeting_id: meeting_id.to_string(),
            stage: ProgressStage::Done,
            fraction: 0.0,
            message,
        },
    );
}
