//! Transcription module.
//!
//! Pipeline (docs/PRD.md §4.5): WAV → whisper.cpp (`whisper-rs`, behind cargo
//! feature `whisper`) → sherpa-onnx diarization (behind cargo feature
//! `diarize`) → `transcript_segments` (incl. speaker). [`model`] handles model
//! download/cache; [`processor`] drives the pipeline + progress reporting.
//!
//! Feature gating
//! --------------
//! The heavy native libraries are optional so the crate builds on a fresh
//! machine without a C++ toolchain or downloaded models:
//! - `whisper` enables the real whisper.cpp transcriber; without it
//!   [`whisper::WhisperTranscriber`] returns a clear "feature not enabled"
//!   error.
//! - `diarize` enables sherpa-onnx speaker diarization; without it the
//!   diarizer is a no-op that leaves every segment's speaker as `None`.
//!
//! Only the pure parts (model registry, segment↔speaker alignment, progress
//! accounting) are unit-tested here; the FFI paths are feature-gated and are
//! noted as unverifiable in this environment.

pub mod diarization;
pub mod model;
pub mod processor;
pub mod whisper;

use std::path::PathBuf;

use thiserror::Error;

/// Errors produced anywhere in the transcription pipeline.
#[derive(Debug, Error)]
pub enum TranscriptionError {
    /// A required cargo feature (e.g. `whisper`) was not compiled in.
    #[error("transcription feature '{0}' is not enabled in this build")]
    FeatureDisabled(&'static str),

    /// An unknown model id was requested from the registry.
    #[error("unknown model id '{0}'")]
    UnknownModel(String),

    /// The model file is not present on disk and could not be (or has not yet
    /// been) downloaded.
    #[error("model '{id}' is not available at {path}")]
    ModelMissing { id: String, path: PathBuf },

    /// Reading/writing the audio (WAV) input failed.
    #[error("audio input error: {0}")]
    Audio(String),

    /// The native whisper/sherpa engine reported a failure.
    #[error("engine error: {0}")]
    Engine(String),

    /// A network/model-download error.
    #[error("download error: {0}")]
    Download(String),

    /// Filesystem error while managing the model cache.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience alias for results in this module.
pub type Result<T> = std::result::Result<T, TranscriptionError>;

/// A coarse phase of the transcription pipeline, surfaced to the UI so the
/// progress bar can show *what* is happening, not just a percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStage {
    /// Downloading / verifying the whisper (and optional diarization) model.
    PreparingModel,
    /// Loading the audio file and decoding/resampling it to 16 kHz mono.
    LoadingAudio,
    /// Running whisper inference over the audio.
    Transcribing,
    /// Running diarization and assigning speaker labels.
    Diarizing,
    /// Pipeline finished; segments are ready to persist.
    Done,
}

/// A single progress update emitted by [`processor::Processor`].
///
/// `fraction` is a monotonically non-decreasing value in `[0.0, 1.0]` spanning
/// the whole pipeline (model prep → audio → whisper → diarization), so a single
/// progress bar can be driven directly from it. `stage` says which phase the
/// fraction currently falls in.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Progress {
    pub stage: ProgressStage,
    /// Overall completion in `[0.0, 1.0]`.
    pub fraction: f32,
    /// Human-readable detail (e.g. "downloading small (466 MB)").
    pub message: String,
}

impl Progress {
    pub fn new(stage: ProgressStage, fraction: f32, message: impl Into<String>) -> Self {
        Progress {
            stage,
            // Clamp defensively so a buggy caller can never drive the UI past
            // 100% or below 0%.
            fraction: fraction.clamp(0.0, 1.0),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_clamps_fraction() {
        assert_eq!(Progress::new(ProgressStage::Done, 1.5, "x").fraction, 1.0);
        assert_eq!(
            Progress::new(ProgressStage::PreparingModel, -0.2, "x").fraction,
            0.0
        );
        assert_eq!(
            Progress::new(ProgressStage::Transcribing, 0.5, "x").fraction,
            0.5
        );
    }

    #[test]
    fn progress_stage_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&ProgressStage::PreparingModel).unwrap(),
            "\"preparing_model\""
        );
        assert_eq!(
            serde_json::to_string(&ProgressStage::Done).unwrap(),
            "\"done\""
        );
    }
}
