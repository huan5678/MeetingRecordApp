//! whisper.cpp transcription via the `whisper-rs` crate, behind cargo feature
//! `whisper`.
//!
//! With the feature **off**, [`WhisperTranscriber::new`] still constructs but
//! [`WhisperTranscriber::transcribe`] returns
//! [`TranscriptionError::FeatureDisabled`], so the rest of the crate compiles
//! and the pipeline degrades with a clear message rather than failing to build.
//!
//! With the feature **on**, it loads a GGML model and runs whisper over a
//! 16 kHz mono `f32` PCM buffer (the format the audio mixer produces for the
//! transcription path, PRD §4.4), yielding timestamped [`RawSegment`]s. The FFI
//! path cannot be exercised in this environment (no native lib / model) and is
//! noted as unverifiable.

use std::path::{Path, PathBuf};

use super::{Result, TranscriptionError};

/// The sample rate whisper.cpp expects. The audio mixer resamples the mix to
/// this rate for the transcription path.
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// One raw segment as emitted by whisper, before diarization. Timings are in
/// milliseconds from the start of the audio.
#[derive(Debug, Clone, PartialEq)]
pub struct RawSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// Whisper's average log-prob mapped to a rough `[0,1]` confidence, when
    /// available.
    pub confidence: Option<f64>,
}

/// Options controlling a whisper run.
#[derive(Debug, Clone)]
pub struct WhisperOptions {
    /// Forced language code (e.g. "zh", "en", "ja"); `None` = auto-detect.
    pub language: Option<String>,
    /// Number of CPU threads. `0` = let whisper pick a default.
    pub threads: u32,
    /// Whether to translate to English instead of transcribing verbatim.
    pub translate: bool,
}

impl Default for WhisperOptions {
    fn default() -> Self {
        WhisperOptions {
            language: None,
            threads: 0,
            translate: false,
        }
    }
}

/// The result of a whisper run: segments plus the detected/forced language.
#[derive(Debug, Clone, PartialEq)]
pub struct WhisperOutput {
    pub segments: Vec<RawSegment>,
    /// The language whisper used (detected or forced).
    pub language: Option<String>,
}

/// Loads a whisper model and transcribes 16 kHz mono PCM.
pub struct WhisperTranscriber {
    model_path: PathBuf,
    #[cfg(feature = "whisper")]
    context: whisper_rs::WhisperContext,
}

impl WhisperTranscriber {
    /// Load the model at `model_path`. Without the `whisper` feature this still
    /// records the path so callers can introspect it, but [`Self::transcribe`]
    /// will return [`TranscriptionError::FeatureDisabled`].
    #[cfg(feature = "whisper")]
    pub fn new(model_path: impl Into<PathBuf>) -> Result<Self> {
        let model_path = model_path.into();
        let path_str = model_path.to_string_lossy().to_string();
        let params = whisper_rs::WhisperContextParameters::default();
        let context = whisper_rs::WhisperContext::new_with_params(&path_str, params)
            .map_err(|e| TranscriptionError::Engine(format!("load model {path_str}: {e}")))?;
        Ok(WhisperTranscriber {
            model_path,
            context,
        })
    }

    /// Stub constructor when the `whisper` feature is off. Records the path but
    /// performs no native loading.
    #[cfg(not(feature = "whisper"))]
    pub fn new(model_path: impl Into<PathBuf>) -> Result<Self> {
        Ok(WhisperTranscriber {
            model_path: model_path.into(),
        })
    }

    /// The model file this transcriber was constructed with.
    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    /// Transcribe a 16 kHz mono `f32` PCM buffer (samples in `[-1.0, 1.0]`).
    ///
    /// `on_segment_progress` is called with a fraction in `[0.0, 1.0]` as
    /// whisper advances through the audio, so the pipeline can fold whisper's
    /// internal progress into the overall progress bar.
    #[cfg(feature = "whisper")]
    pub fn transcribe(
        &self,
        pcm_16k_mono: &[f32],
        options: &WhisperOptions,
        mut on_segment_progress: impl FnMut(f32),
    ) -> Result<WhisperOutput> {
        use whisper_rs::{FullParams, SamplingStrategy};

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        if let Some(lang) = options.language.as_deref() {
            params.set_language(Some(lang));
        }
        params.set_translate(options.translate);
        if options.threads > 0 {
            params.set_n_threads(options.threads as i32);
        }
        // We surface our own staged progress; silence whisper's stdout chatter.
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        let mut state = self
            .context
            .create_state()
            .map_err(|e| TranscriptionError::Engine(format!("create state: {e}")))?;

        state
            .full(params, pcm_16k_mono)
            .map_err(|e| TranscriptionError::Engine(format!("whisper full(): {e}")))?;

        let n = state
            .full_n_segments()
            .map_err(|e| TranscriptionError::Engine(format!("n_segments: {e}")))?;

        let mut segments = Vec::with_capacity(n as usize);
        for i in 0..n {
            let text = state
                .full_get_segment_text(i)
                .map_err(|e| TranscriptionError::Engine(format!("segment {i} text: {e}")))?;
            // whisper timestamps are in centiseconds (1/100 s).
            let start_cs = state
                .full_get_segment_t0(i)
                .map_err(|e| TranscriptionError::Engine(format!("segment {i} t0: {e}")))?;
            let end_cs = state
                .full_get_segment_t1(i)
                .map_err(|e| TranscriptionError::Engine(format!("segment {i} t1: {e}")))?;
            segments.push(RawSegment {
                start_ms: start_cs * 10,
                end_ms: end_cs * 10,
                text: text.trim().to_string(),
                confidence: None,
            });
            if n > 0 {
                on_segment_progress((i + 1) as f32 / n as f32);
            }
        }
        on_segment_progress(1.0);

        Ok(WhisperOutput {
            segments,
            language: options.language.clone(),
        })
    }

    /// Stub: without the `whisper` feature there is no engine to call.
    #[cfg(not(feature = "whisper"))]
    pub fn transcribe(
        &self,
        _pcm_16k_mono: &[f32],
        _options: &WhisperOptions,
        _on_segment_progress: impl FnMut(f32),
    ) -> Result<WhisperOutput> {
        Err(TranscriptionError::FeatureDisabled("whisper"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rate_is_16k() {
        assert_eq!(WHISPER_SAMPLE_RATE, 16_000);
    }

    #[test]
    fn options_default_auto_language() {
        let o = WhisperOptions::default();
        assert!(o.language.is_none());
        assert!(!o.translate);
        assert_eq!(o.threads, 0);
    }

    // The transcriber constructs even without the native lib (stub path) so the
    // pipeline can introspect the model path; transcribe() then reports the
    // feature is disabled. The real FFI path is unverifiable here.
    #[cfg(not(feature = "whisper"))]
    #[test]
    fn transcribe_without_feature_reports_disabled() {
        let t = WhisperTranscriber::new("/models/ggml-small.bin").unwrap();
        assert_eq!(t.model_path().to_str().unwrap(), "/models/ggml-small.bin");
        let err = t
            .transcribe(&[0.0f32; 16], &WhisperOptions::default(), |_| {})
            .unwrap_err();
        match err {
            TranscriptionError::FeatureDisabled(f) => assert_eq!(f, "whisper"),
            other => panic!("expected FeatureDisabled, got {other:?}"),
        }
    }
}
