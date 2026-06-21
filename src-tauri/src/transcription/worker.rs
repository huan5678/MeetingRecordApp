//! Transcription worker (Phase 3 wiring).
//!
//! Runs the [`Processor`](super::processor::Processor) pipeline on a dedicated
//! thread so the (CPU-heavy, possibly long) whisper + diarization work never
//! blocks Tauri's IPC thread. Progress is surfaced two ways:
//!
//! 1. **Polled** — written into [`AppState::transcription`], which the existing
//!    `get_transcription_status` command reads. This is what the current
//!    frontend (`useTranscription`, a poller per PRD §3.3 #15) consumes.
//! 2. **Pushed** — emitted as Tauri events (`transcription://progress` /
//!    `://done` / `://error`) so a future live UI can subscribe without polling.
//!
//! On a build without the `whisper` cargo feature,
//! [`Processor::transcribe_wav`](super::processor::Processor::transcribe_wav)
//! returns `FeatureDisabled` immediately; the worker records that as an error
//! with a clear "build with --features whisper" message and flips the meeting to
//! `Error`. With the feature + a cached model it does the real work and persists
//! the segments.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager};

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

/// Spawn the transcription pipeline for `meeting_id` over `wav_path`. Returns
/// immediately; progress + completion are reported via the app state map and
/// Tauri events. `model_id` selects the whisper model (falls back to the default
/// `small` if unknown); `diarize` toggles speaker labelling.
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
    let processor = Processor::new(manager);
    let created_at = crate::commands::now_iso8601();
    let options = ProcessorOptions {
        diarize,
        ..Default::default()
    };

    let state = app.state::<AppState>();
    let progress_app = app.clone();
    let progress_id = meeting_id.clone();

    let result = processor.transcribe_wav(
        &wav_path,
        model,
        &meeting_id,
        &created_at,
        &options,
        |p: Progress| {
            if let Ok(mut map) = state.transcription.lock() {
                map.insert(
                    progress_id.clone(),
                    TranscriptionProgressEntry {
                        stage: stage_str(p.stage).to_string(),
                        fraction: p.fraction,
                        message: Some(p.message.clone()),
                    },
                );
            }
            let _ = progress_app.emit(
                "transcription://progress",
                ProgressEvent {
                    meeting_id: progress_id.clone(),
                    stage: p.stage,
                    fraction: p.fraction,
                    message: p.message,
                },
            );
        },
    );

    match result {
        Ok(segments) => {
            if let Ok(db) = state.db.lock() {
                let _ = db.insert_transcript_segments(&segments);
                let _ = db.set_meeting_status(&meeting_id, MeetingStatus::Completed);
            }
            if let Ok(mut map) = state.transcription.lock() {
                map.insert(
                    meeting_id.clone(),
                    TranscriptionProgressEntry {
                        stage: "done".to_string(),
                        fraction: 1.0,
                        message: Some(format!("{} segments", segments.len())),
                    },
                );
            }
            let _ = app.emit(
                "transcription://done",
                ProgressEvent {
                    meeting_id,
                    stage: ProgressStage::Done,
                    fraction: 1.0,
                    message: format!("{} segments", segments.len()),
                },
            );
        }
        Err(e) => {
            let message = e.to_string();
            // A build without the `whisper` feature is a capability gap, not a
            // failed recording: the audio is fine, there's just no transcript.
            // Mark such meetings Completed so they don't show as errors.
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
