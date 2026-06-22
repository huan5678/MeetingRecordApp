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

use crate::commands::{AppState, TranscriptionProgressEntry};
use crate::models::MeetingStatus;

use super::model::{default_model, lookup, ModelManager};
use super::processor::{Processor, ProcessorOptions};
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
pub fn spawn_transcription(
    app: AppHandle,
    meeting_id: String,
    wav_path: PathBuf,
    model_id: String,
    diarize: bool,
) {
    let _ = std::thread::Builder::new()
        .name("transcription".into())
        .spawn(move || run(app, meeting_id, wav_path, model_id, diarize));
}

fn run(app: AppHandle, meeting_id: String, wav_path: PathBuf, model_id: String, diarize: bool) {
    let cache_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("models");
    let manager = ModelManager::new(cache_dir);
    let model = lookup(&model_id).unwrap_or_else(|_| default_model());
    let state = app.state::<AppState>();

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
    let options = ProcessorOptions {
        diarize,
        ..Default::default()
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
            if let Ok(db) = state.db.lock() {
                let _ = db.insert_transcript_segments(&segments);
                let _ = db.set_meeting_status(&meeting_id, MeetingStatus::Completed);
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
#[cfg(feature = "whisper")]
fn finish_error(app: &AppHandle, state: &State<'_, AppState>, meeting_id: &str, message: String) {
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
