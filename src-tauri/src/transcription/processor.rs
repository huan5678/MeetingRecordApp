//! Transcription pipeline driver (PRD §4.5).
//!
//! ```text
//! WAV file
//!   → decode + resample to 16 kHz mono f32 PCM
//!   → whisper (RawSegment[])
//!   → diarization (SpeakerTurn[])  → align speakers onto segments
//!   → Vec<TranscriptSegment> (incl. speaker), with progress events
//! ```
//!
//! Two concerns are kept apart so the logic is testable without native libs:
//! - [`ProgressAccountant`] is the **pure** progress-weighting machine that
//!   folds each stage's local `[0,1]` progress into one overall `[0,1]`
//!   fraction. Fully unit-tested.
//! - [`build_segments`] is the **pure** conversion from raw whisper segments +
//!   per-segment speaker labels into persistable [`TranscriptSegment`] rows.
//!   Fully unit-tested with fake data.
//! - [`Processor::transcribe_wav`] wires the (feature-gated) whisper +
//!   diarization engines together and emits [`Progress`] events. Its native
//!   parts are unverifiable here.

use crate::models::TranscriptSegment;

#[cfg(feature = "whisper")]
use super::diarization::{assign_speakers, Diarizer};
use super::model::{ModelInfo, ModelManager};
use super::whisper::{RawSegment, WhisperOptions, WHISPER_SAMPLE_RATE};
#[cfg(feature = "whisper")]
use super::whisper::WhisperTranscriber;
use super::{Progress, ProgressStage, Result, TranscriptionError};

/// Relative weights of each pipeline stage in the overall progress bar. They
/// need not sum to 1.0 — [`ProgressAccountant`] normalizes them — but keeping
/// them readable (model prep is cheap once cached, whisper dominates) makes the
/// bar feel honest.
const W_PREPARING_MODEL: f32 = 0.05;
const W_LOADING_AUDIO: f32 = 0.05;
const W_TRANSCRIBING: f32 = 0.80;
const W_DIARIZING: f32 = 0.10;

/// Folds per-stage progress into a single monotonically non-decreasing overall
/// fraction. Pure and deterministic.
#[derive(Debug, Clone)]
pub struct ProgressAccountant {
    /// (stage, weight) in pipeline order. Weights are normalized on use.
    weights: Vec<(ProgressStage, f32)>,
    total_weight: f32,
}

impl Default for ProgressAccountant {
    fn default() -> Self {
        ProgressAccountant::new(vec![
            (ProgressStage::PreparingModel, W_PREPARING_MODEL),
            (ProgressStage::LoadingAudio, W_LOADING_AUDIO),
            (ProgressStage::Transcribing, W_TRANSCRIBING),
            (ProgressStage::Diarizing, W_DIARIZING),
        ])
    }
}

impl ProgressAccountant {
    pub fn new(weights: Vec<(ProgressStage, f32)>) -> Self {
        let total_weight = weights.iter().map(|(_, w)| *w).sum::<f32>().max(f32::EPSILON);
        ProgressAccountant {
            weights,
            total_weight,
        }
    }

    /// Sum of normalized weights of all stages strictly *before* `stage`.
    fn base_fraction(&self, stage: ProgressStage) -> f32 {
        let mut acc = 0.0;
        for (s, w) in &self.weights {
            if *s == stage {
                break;
            }
            acc += w / self.total_weight;
        }
        acc
    }

    /// Normalized weight of `stage` itself.
    fn stage_weight(&self, stage: ProgressStage) -> f32 {
        self.weights
            .iter()
            .find(|(s, _)| *s == stage)
            .map(|(_, w)| w / self.total_weight)
            .unwrap_or(0.0)
    }

    /// Overall fraction in `[0,1]` given that `stage` is `local` (in `[0,1]`)
    /// complete.
    pub fn overall(&self, stage: ProgressStage, local: f32) -> f32 {
        if stage == ProgressStage::Done {
            return 1.0;
        }
        let local = local.clamp(0.0, 1.0);
        (self.base_fraction(stage) + self.stage_weight(stage) * local).clamp(0.0, 1.0)
    }

    /// Build a [`Progress`] event for `stage` at `local` completion.
    pub fn progress(
        &self,
        stage: ProgressStage,
        local: f32,
        message: impl Into<String>,
    ) -> Progress {
        Progress::new(stage, self.overall(stage, local), message)
    }
}

/// Convert whisper raw segments + aligned speaker labels into persistable
/// [`TranscriptSegment`] rows.
///
/// Pure: the caller supplies the `meeting_id`, a `new_id` factory (so tests can
/// inject deterministic ids instead of random UUIDs), the detected `language`,
/// and a `created_at` timestamp string. `speakers[i]` is the label for
/// `raw[i]`; its length must equal `raw.len()` (guaranteed by
/// [`assign_speakers`]).
pub fn build_segments(
    meeting_id: &str,
    raw: &[RawSegment],
    speakers: &[Option<String>],
    language: Option<&str>,
    created_at: &str,
    mut new_id: impl FnMut() -> String,
) -> Vec<TranscriptSegment> {
    debug_assert_eq!(raw.len(), speakers.len());
    raw.iter()
        .enumerate()
        .map(|(i, s)| TranscriptSegment {
            id: new_id(),
            meeting_id: meeting_id.to_string(),
            segment_index: i as i64,
            start_time_ms: s.start_ms,
            end_time_ms: s.end_ms,
            text: s.text.clone(),
            speaker: speakers.get(i).cloned().flatten(),
            confidence: s.confidence,
            language: language.map(|l| l.to_string()),
            created_at: created_at.to_string(),
        })
        .collect()
}

/// Options for one pipeline run.
#[derive(Debug, Clone)]
pub struct ProcessorOptions {
    pub whisper: WhisperOptions,
    /// Run diarization (requires the `diarize` feature to do anything useful).
    pub diarize: bool,
}

impl Default for ProcessorOptions {
    fn default() -> Self {
        ProcessorOptions {
            whisper: WhisperOptions::default(),
            diarize: true,
        }
    }
}

/// Drives the WAV → transcript pipeline.
// Both fields are only read by the `whisper`-gated pipeline; on default builds
// they're constructed but unused.
#[cfg_attr(not(feature = "whisper"), allow(dead_code))]
pub struct Processor {
    model_manager: ModelManager,
    accountant: ProgressAccountant,
}

impl Processor {
    pub fn new(model_manager: ModelManager) -> Self {
        Processor {
            model_manager,
            accountant: ProgressAccountant::default(),
        }
    }

    /// Decode a mono/stereo WAV file into 16 kHz mono `f32` PCM for whisper.
    ///
    /// Uses `hound` to read the WAV. If the source is multi-channel it is
    /// downmixed to mono by averaging; if its sample rate differs from 16 kHz it
    /// is resampled with a simple linear interpolator. This is the only place
    /// the pipeline touches the audio container; the heavier mixer/resampler
    /// lives in the `audio` module for the live path.
    pub fn load_wav_16k_mono(path: &std::path::Path) -> Result<Vec<f32>> {
        let mut reader = hound::WavReader::open(path)
            .map_err(|e| TranscriptionError::Audio(format!("open {}: {e}", path.display())))?;
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;

        // Read all samples as f32 in [-1, 1], regardless of int/float encoding.
        let interleaved: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| TranscriptionError::Audio(format!("read float samples: {e}")))?,
            hound::SampleFormat::Int => {
                let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 / max))
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| TranscriptionError::Audio(format!("read int samples: {e}")))?
            }
        };

        let mono = downmix_to_mono(&interleaved, channels);
        let resampled = resample_linear(&mono, spec.sample_rate, WHISPER_SAMPLE_RATE);
        Ok(resampled)
    }

    /// Full pipeline: load WAV → whisper → diarize → aligned segments, emitting
    /// progress along the way.
    ///
    /// `model` selects the whisper model (must already be cached — the caller
    /// downloads it via [`ModelManager::ensure_model`] first so this method
    /// stays sync-friendly). `on_progress` receives staged [`Progress`] events
    /// whose `fraction` rises monotonically from 0 to 1.
    #[cfg(feature = "whisper")]
    pub fn transcribe_wav(
        &self,
        wav_path: &std::path::Path,
        model: &ModelInfo,
        meeting_id: &str,
        created_at: &str,
        options: &ProcessorOptions,
        mut on_progress: impl FnMut(Progress),
    ) -> Result<Vec<TranscriptSegment>> {
        let acct = &self.accountant;

        // 1. Model: ensure the chosen whisper model is on disk.
        on_progress(acct.progress(ProgressStage::PreparingModel, 0.0, "checking model"));
        let model_path = self.model_manager.require_cached(model)?;
        on_progress(acct.progress(
            ProgressStage::PreparingModel,
            1.0,
            format!("model ready: {}", model.id),
        ));

        // 2. Audio: decode + resample.
        on_progress(acct.progress(ProgressStage::LoadingAudio, 0.0, "loading audio"));
        let pcm = Self::load_wav_16k_mono(wav_path)?;
        on_progress(acct.progress(ProgressStage::LoadingAudio, 1.0, "audio ready"));

        // 3. Whisper.
        let transcriber = WhisperTranscriber::new(model_path)?;
        let output = transcriber.transcribe(&pcm, &options.whisper, |local| {
            on_progress(acct.progress(ProgressStage::Transcribing, local, "transcribing"));
        })?;

        // 4. Diarization (no-op without the `diarize` feature).
        let speakers = if options.diarize {
            on_progress(acct.progress(ProgressStage::Diarizing, 0.0, "diarizing"));
            let turns = self.run_diarization(&pcm)?;
            let labels = assign_speakers(&output.segments, &turns);
            on_progress(acct.progress(ProgressStage::Diarizing, 1.0, "speakers assigned"));
            labels
        } else {
            vec![None; output.segments.len()]
        };

        // 5. Build rows.
        let segments = build_segments(
            meeting_id,
            &output.segments,
            &speakers,
            output.language.as_deref(),
            created_at,
            || uuid::Uuid::new_v4().to_string(),
        );

        on_progress(Progress::new(ProgressStage::Done, 1.0, "done"));
        Ok(segments)
    }

    /// Stub pipeline when whisper is not compiled in: fail fast with a clear
    /// message so the UI can tell the user to build with `--features whisper`.
    #[cfg(not(feature = "whisper"))]
    pub fn transcribe_wav(
        &self,
        _wav_path: &std::path::Path,
        _model: &ModelInfo,
        _meeting_id: &str,
        _created_at: &str,
        _options: &ProcessorOptions,
        _on_progress: impl FnMut(Progress),
    ) -> Result<Vec<TranscriptSegment>> {
        Err(TranscriptionError::FeatureDisabled("whisper"))
    }

    /// Build a diarizer from the model cache and run it. Without the `diarize`
    /// feature the diarizer yields no turns, so this returns an empty Vec.
    #[cfg(feature = "whisper")]
    fn run_diarization(&self, pcm: &[f32]) -> Result<Vec<super::diarization::SpeakerTurn>> {
        let cache = self.model_manager.cache_dir();
        let diarizer = Diarizer::new(super::diarization::DiarizeConfig {
            segmentation_model: cache.join("sherpa-segmentation.onnx"),
            embedding_model: cache.join("sherpa-embedding.onnx"),
            num_speakers: None,
        });
        diarizer.diarize(pcm)
    }
}

/// Average interleaved samples down to a single mono channel.
fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Resample a mono signal from `from_rate` to `to_rate` by linear
/// interpolation. Good enough for whisper's 16 kHz front-end; the audio module
/// owns the high-quality live-path resampler.
fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let a = input.get(idx).copied().unwrap_or(0.0);
        let b = input.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(start_ms: i64, end_ms: i64, text: &str) -> RawSegment {
        RawSegment {
            start_ms,
            end_ms,
            text: text.into(),
            confidence: Some(0.9),
        }
    }

    // ---- ProgressAccountant ------------------------------------------------

    #[test]
    fn accountant_stage_boundaries_are_cumulative() {
        let a = ProgressAccountant::default();
        // Default weights: 0.05, 0.05, 0.80, 0.10 (sum 1.0).
        assert!((a.overall(ProgressStage::PreparingModel, 0.0) - 0.0).abs() < 1e-6);
        assert!((a.overall(ProgressStage::PreparingModel, 1.0) - 0.05).abs() < 1e-6);
        assert!((a.overall(ProgressStage::LoadingAudio, 0.0) - 0.05).abs() < 1e-6);
        assert!((a.overall(ProgressStage::LoadingAudio, 1.0) - 0.10).abs() < 1e-6);
        assert!((a.overall(ProgressStage::Transcribing, 0.0) - 0.10).abs() < 1e-6);
        assert!((a.overall(ProgressStage::Transcribing, 0.5) - 0.50).abs() < 1e-6);
        assert!((a.overall(ProgressStage::Transcribing, 1.0) - 0.90).abs() < 1e-6);
        assert!((a.overall(ProgressStage::Diarizing, 1.0) - 1.0).abs() < 1e-6);
        assert_eq!(a.overall(ProgressStage::Done, 0.0), 1.0);
    }

    #[test]
    fn accountant_is_monotonic_across_stages() {
        let a = ProgressAccountant::default();
        let sequence = [
            a.overall(ProgressStage::PreparingModel, 0.0),
            a.overall(ProgressStage::PreparingModel, 1.0),
            a.overall(ProgressStage::LoadingAudio, 0.5),
            a.overall(ProgressStage::LoadingAudio, 1.0),
            a.overall(ProgressStage::Transcribing, 0.25),
            a.overall(ProgressStage::Transcribing, 0.75),
            a.overall(ProgressStage::Transcribing, 1.0),
            a.overall(ProgressStage::Diarizing, 0.5),
            a.overall(ProgressStage::Diarizing, 1.0),
            a.overall(ProgressStage::Done, 0.0),
        ];
        for w in sequence.windows(2) {
            assert!(w[1] >= w[0], "progress went backwards: {w:?}");
        }
        assert_eq!(*sequence.last().unwrap(), 1.0);
    }

    #[test]
    fn accountant_clamps_local_progress() {
        let a = ProgressAccountant::default();
        // local > 1 must not exceed the stage's upper bound.
        assert!((a.overall(ProgressStage::Transcribing, 2.0) - 0.90).abs() < 1e-6);
        assert!((a.overall(ProgressStage::Transcribing, -1.0) - 0.10).abs() < 1e-6);
    }

    #[test]
    fn accountant_normalizes_arbitrary_weights() {
        // Weights that don't sum to 1 are normalized.
        let a = ProgressAccountant::new(vec![
            (ProgressStage::Transcribing, 30.0),
            (ProgressStage::Diarizing, 10.0),
        ]);
        assert!((a.overall(ProgressStage::Transcribing, 1.0) - 0.75).abs() < 1e-6);
        assert!((a.overall(ProgressStage::Diarizing, 1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn accountant_progress_event_carries_stage_and_overall() {
        let a = ProgressAccountant::default();
        let p = a.progress(ProgressStage::Transcribing, 0.5, "go");
        assert_eq!(p.stage, ProgressStage::Transcribing);
        assert!((p.fraction - 0.5).abs() < 1e-6);
        assert_eq!(p.message, "go");
    }

    // ---- build_segments ----------------------------------------------------

    #[test]
    fn build_segments_indexes_and_copies_fields() {
        let raw = vec![raw(0, 1000, "hello"), raw(1000, 2000, "world")];
        let speakers = vec![Some("Speaker 1".to_string()), None];
        let mut counter = 0;
        let out = build_segments(
            "meeting-x",
            &raw,
            &speakers,
            Some("en"),
            "2026-06-19T00:00:00Z",
            || {
                counter += 1;
                format!("seg-{counter}")
            },
        );
        assert_eq!(out.len(), 2);

        assert_eq!(out[0].id, "seg-1");
        assert_eq!(out[0].meeting_id, "meeting-x");
        assert_eq!(out[0].segment_index, 0);
        assert_eq!(out[0].start_time_ms, 0);
        assert_eq!(out[0].end_time_ms, 1000);
        assert_eq!(out[0].text, "hello");
        assert_eq!(out[0].speaker.as_deref(), Some("Speaker 1"));
        assert_eq!(out[0].confidence, Some(0.9));
        assert_eq!(out[0].language.as_deref(), Some("en"));
        assert_eq!(out[0].created_at, "2026-06-19T00:00:00Z");

        assert_eq!(out[1].id, "seg-2");
        assert_eq!(out[1].segment_index, 1);
        assert_eq!(out[1].speaker, None);
    }

    #[test]
    fn build_segments_empty_input_yields_empty() {
        let out = build_segments("m", &[], &[], None, "now", || "x".to_string());
        assert!(out.is_empty());
    }

    #[test]
    fn build_segments_language_none_propagates() {
        let raw = vec![raw(0, 500, "x")];
        let speakers = vec![None];
        let out = build_segments("m", &raw, &speakers, None, "now", || "id".to_string());
        assert_eq!(out[0].language, None);
    }

    // ---- audio helpers -----------------------------------------------------

    #[test]
    fn downmix_stereo_averages_channels() {
        // L,R interleaved: (1,-1),(0.5,0.5) → 0.0, 0.5
        let interleaved = vec![1.0, -1.0, 0.5, 0.5];
        assert_eq!(downmix_to_mono(&interleaved, 2), vec![0.0, 0.5]);
    }

    #[test]
    fn downmix_mono_is_identity() {
        let mono = vec![0.1, 0.2, 0.3];
        assert_eq!(downmix_to_mono(&mono, 1), mono);
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let s = vec![0.0, 0.5, 1.0];
        assert_eq!(resample_linear(&s, 16_000, 16_000), s);
    }

    #[test]
    fn resample_downsamples_length() {
        // 32k → 16k halves the sample count (approximately).
        let input: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let out = resample_linear(&input, 32_000, 16_000);
        assert_eq!(out.len(), 50);
        // First sample preserved.
        assert!((out[0] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn resample_upsamples_length() {
        let input = vec![0.0, 1.0];
        let out = resample_linear(&input, 8_000, 16_000);
        assert_eq!(out.len(), 4);
        // Linear interpolation between 0 and 1.
        assert!(out[0] <= out[1] && out[1] <= out[out.len() - 1]);
    }

    #[test]
    fn resample_empty_is_empty() {
        assert!(resample_linear(&[], 8_000, 16_000).is_empty());
    }

    // ---- end-to-end alignment (pure parts composed) ------------------------

    #[test]
    fn segments_compose_with_assign_speakers() {
        use super::super::diarization::{assign_speakers, SpeakerTurn};
        let raw = vec![raw(0, 1000, "a"), raw(1000, 2000, "b")];
        let turns = vec![
            SpeakerTurn {
                start_ms: 0,
                end_ms: 1000,
                speaker_id: 0,
            },
            SpeakerTurn {
                start_ms: 1000,
                end_ms: 2000,
                speaker_id: 1,
            },
        ];
        let speakers = assign_speakers(&raw, &turns);
        let out = build_segments("m", &raw, &speakers, Some("en"), "now", || "id".into());
        assert_eq!(out[0].speaker.as_deref(), Some("Speaker 1"));
        assert_eq!(out[1].speaker.as_deref(), Some("Speaker 2"));
    }

    #[cfg(not(feature = "whisper"))]
    #[test]
    fn transcribe_wav_without_feature_is_disabled() {
        let proc = Processor::new(ModelManager::new("/tmp/models"));
        let model = super::super::model::default_model();
        let err = proc
            .transcribe_wav(
                std::path::Path::new("/tmp/x.wav"),
                model,
                "m",
                "now",
                &ProcessorOptions::default(),
                |_| {},
            )
            .unwrap_err();
        assert!(matches!(err, TranscriptionError::FeatureDisabled("whisper")));
    }
}
