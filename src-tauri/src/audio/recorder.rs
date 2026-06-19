//! Recording orchestrator.
//!
//! Owns the lifecycle (`start` → `pause`/`resume` ↔ → `stop`), drives the two
//! capture legs into the [`mixer`](super::mixer), writes the mixed audio to WAV
//! via [`hound`], and tracks recorded duration.
//!
//! Design notes
//! ------------
//! * **State machine.** [`RecorderState`] is a small, explicit FSM. Illegal
//!   transitions (resume-while-recording, stop-while-idle, …) return
//!   [`AudioError::InvalidState`] instead of silently corrupting a file. The
//!   FSM is pure and exhaustively tested.
//! * **Duration from samples, not wall clock.** We count *frames actually
//!   written* and divide by the sample rate. Time spent paused contributes no
//!   frames, so a pause can never inflate the duration or leave a silent gap in
//!   the file (PRD §3.1 user stories 3 & 4). This is the key correctness
//!   property and it is unit-tested.
//! * **Drift + buffering** live in the mixer; the recorder wires a
//!   [`DriftCompensator`] and [`SampleQueue`] per leg and only feeds the
//!   *combined* output to the WAV writer.
//!
//! The live capture wiring (spawning mic/system streams) needs real devices and
//! is gated; the file-writing and accounting paths are pure and tested here.

use std::path::{Path, PathBuf};

use super::mixer::{self, f32_to_i16, DriftCompensator, MixOutput, SampleQueue};
use super::{AudioError, Result, DEFAULT_RECORD_SAMPLE_RATE, WHISPER_SAMPLE_RATE};

/// Lifecycle states of the recorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderState {
    /// Constructed but not started.
    Idle,
    /// Actively capturing and writing.
    Recording,
    /// Capture suspended; the WAV is held open, duration frozen.
    Paused,
    /// Finalised; the WAV is closed. Terminal.
    Stopped,
}

/// Configuration for a recording session.
#[derive(Debug, Clone)]
pub struct RecorderConfig {
    /// Sample rate of the main recording WAV (PRD: 44.1 kHz recording vs 16 kHz
    /// for whisper). We resample legs to this common rate before summing.
    pub record_sample_rate: u32,
    /// Whether to capture the system-audio loopback leg (Windows-only).
    pub capture_system: bool,
    /// Whether to capture the microphone leg.
    pub capture_microphone: bool,
    /// Keep a separate system/mic dual-track for diarization hints (PRD §4.4).
    pub keep_dual_track: bool,
    /// Per-leg ring-buffer capacity in samples (over/under-run bound).
    pub queue_capacity: usize,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            record_sample_rate: DEFAULT_RECORD_SAMPLE_RATE,
            capture_system: true,
            capture_microphone: true,
            keep_dual_track: false,
            queue_capacity: DEFAULT_RECORD_SAMPLE_RATE as usize, // ~1s of mono
        }
    }
}

/// Minimal WAV sink abstraction so accounting/state tests don't need a real
/// file. The production impl wraps [`hound::WavWriter`].
pub trait WavSink {
    /// Append mono `i16` samples; returns the number written.
    fn write_samples(&mut self, samples: &[i16]) -> Result<usize>;
    /// Flush and finalise the file.
    fn finalize(self: Box<Self>) -> Result<()>;
}

/// `hound`-backed WAV writer producing 16-bit mono PCM.
pub struct HoundWavSink {
    writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
}

impl HoundWavSink {
    /// Create a 16-bit mono PCM WAV at `path` with sample rate `rate`.
    pub fn create(path: &Path, rate: u32) -> Result<Self> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(path, spec)?;
        Ok(Self { writer })
    }
}

impl WavSink for HoundWavSink {
    fn write_samples(&mut self, samples: &[i16]) -> Result<usize> {
        for &s in samples {
            self.writer.write_sample(s)?;
        }
        Ok(samples.len())
    }

    fn finalize(self: Box<Self>) -> Result<()> {
        self.writer.finalize()?;
        Ok(())
    }
}

/// Orchestrates a single recording. Generic over the WAV sink so the FSM and
/// frame-accounting can be tested against an in-memory fake.
pub struct Recorder<S: WavSink> {
    config: RecorderConfig,
    state: RecorderState,
    sink: Option<Box<S>>,
    /// Path of the main WAV (for reporting back to storage).
    output_path: PathBuf,
    /// Total mono frames written to the *recording* WAV (excludes paused time).
    frames_written: u64,
    /// Drift compensators, one per leg, persisted across chunks.
    system_drift: DriftCompensator,
    mic_drift: DriftCompensator,
    /// Per-leg jitter buffers (mixer handles under/overrun). Cleared on pause
    /// so resume starts clean; the live capture threads (wired in Integrate)
    /// push/pop through these between the device callbacks and `feed`.
    system_queue: SampleQueue,
    mic_queue: SampleQueue,
    /// Accumulated transcription-path samples (16 kHz mono) handed to whisper
    /// when recording stops. Kept in-memory; a long meeting at 16 kHz mono is
    /// ~115 MB/hr of f32, acceptable for v1.0 (PRD §7.1 disk budget).
    transcription_samples: Vec<f32>,
    /// Optional dual-track accumulation `(system, mic)` for diarization.
    dual_track: Option<(Vec<f32>, Vec<f32>)>,
}

impl<S: WavSink> Recorder<S> {
    /// Build a recorder around an already-created sink (e.g. [`HoundWavSink`]).
    /// Starts in [`RecorderState::Idle`]; call [`start`](Self::start) to begin.
    pub fn new(config: RecorderConfig, output_path: PathBuf, sink: S) -> Self {
        let qcap = config.queue_capacity;
        let rate = config.record_sample_rate;
        let dual = if config.keep_dual_track {
            Some((Vec::new(), Vec::new()))
        } else {
            None
        };
        Self {
            config,
            state: RecorderState::Idle,
            sink: Some(Box::new(sink)),
            output_path,
            frames_written: 0,
            system_drift: DriftCompensator::new(rate, 0.1, 0.02),
            mic_drift: DriftCompensator::new(rate, 0.1, 0.02),
            system_queue: SampleQueue::new(qcap),
            mic_queue: SampleQueue::new(qcap),
            transcription_samples: Vec::new(),
            dual_track: dual,
        }
    }

    pub fn state(&self) -> RecorderState {
        self.state
    }

    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    /// Drift compensators are exposed so the live capture threads can `observe`
    /// produced frames against wall-clock time.
    pub fn system_drift_mut(&mut self) -> &mut DriftCompensator {
        &mut self.system_drift
    }

    pub fn mic_drift_mut(&mut self) -> &mut DriftCompensator {
        &mut self.mic_drift
    }

    /// Recorded duration in milliseconds, derived from frames actually written
    /// (paused time excluded — see module docs).
    pub fn duration_ms(&self) -> u64 {
        let rate = self.config.record_sample_rate.max(1) as u64;
        self.frames_written * 1000 / rate
    }

    /// Recorded duration in whole seconds (what `meetings.duration_seconds`
    /// stores).
    pub fn duration_seconds(&self) -> i64 {
        (self.frames_written / self.config.record_sample_rate.max(1) as u64) as i64
    }

    // ---- state transitions ------------------------------------------------

    /// Idle → Recording.
    pub fn start(&mut self) -> Result<()> {
        match self.state {
            RecorderState::Idle => {
                self.state = RecorderState::Recording;
                Ok(())
            }
            other => Err(AudioError::InvalidState(format!(
                "start requires Idle, was {other:?}"
            ))),
        }
    }

    /// Recording → Paused. While paused, [`feed`](Self::feed) is rejected so no
    /// frames are written and the duration is frozen.
    pub fn pause(&mut self) -> Result<()> {
        match self.state {
            RecorderState::Recording => {
                self.state = RecorderState::Paused;
                // Drop any queued-but-unwritten capture so resume starts clean
                // and no stale pre-pause audio bleeds across the gap.
                self.system_queue.clear();
                self.mic_queue.clear();
                Ok(())
            }
            other => Err(AudioError::InvalidState(format!(
                "pause requires Recording, was {other:?}"
            ))),
        }
    }

    /// Paused → Recording.
    pub fn resume(&mut self) -> Result<()> {
        match self.state {
            RecorderState::Paused => {
                self.state = RecorderState::Recording;
                Ok(())
            }
            other => Err(AudioError::InvalidState(format!(
                "resume requires Paused, was {other:?}"
            ))),
        }
    }

    /// Feed one (optional) chunk per leg into the mixer and write the combined
    /// mono result to the WAV. No-op (and no frame advance) while paused/idle.
    ///
    /// Returns the number of mono frames written to the recording WAV for this
    /// call (0 while paused).
    pub fn feed(
        &mut self,
        system: Option<&super::AudioChunk>,
        mic: Option<&super::AudioChunk>,
    ) -> Result<usize> {
        if self.state != RecorderState::Recording {
            // Paused/Idle/Stopped: silently ignore so a late callback after
            // pause cannot corrupt the file or duration.
            return Ok(0);
        }

        let out: MixOutput = mixer::mix_chunks(
            system,
            mic,
            self.config.record_sample_rate,
            &self.system_drift,
            &self.mic_drift,
            self.config.keep_dual_track,
        );

        // Write recording (mono i16) to WAV.
        let pcm = f32_to_i16(&out.recording);
        let written = match self.sink.as_mut() {
            Some(s) => s.write_samples(&pcm)?,
            None => return Err(AudioError::InvalidState("sink already finalized".into())),
        };
        self.frames_written += written as u64;

        // Accumulate the 16 kHz transcription stream.
        self.transcription_samples
            .extend_from_slice(&out.transcription);

        // Accumulate dual-track if enabled.
        if let (Some(dst), Some((s, m))) = (self.dual_track.as_mut(), out.dual_track) {
            dst.0.extend_from_slice(&s);
            dst.1.extend_from_slice(&m);
        }

        Ok(written)
    }

    /// Recording|Paused → Stopped. Finalises the WAV and returns a summary.
    pub fn stop(&mut self) -> Result<RecordingResult> {
        match self.state {
            RecorderState::Recording | RecorderState::Paused => {
                if let Some(sink) = self.sink.take() {
                    sink.finalize()?;
                }
                self.state = RecorderState::Stopped;
                Ok(RecordingResult {
                    wav_path: self.output_path.clone(),
                    duration_ms: self.duration_ms(),
                    frames: self.frames_written,
                    record_sample_rate: self.config.record_sample_rate,
                    transcription_sample_rate: WHISPER_SAMPLE_RATE,
                    transcription_samples: std::mem::take(&mut self.transcription_samples),
                    dual_track: self.dual_track.take(),
                })
            }
            other => Err(AudioError::InvalidState(format!(
                "stop requires Recording or Paused, was {other:?}"
            ))),
        }
    }
}

/// What a finished recording yields. The transcription samples (16 kHz mono)
/// are handed straight to the whisper path; `wav_path` is registered in
/// `media_files`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingResult {
    pub wav_path: PathBuf,
    pub duration_ms: u64,
    pub frames: u64,
    pub record_sample_rate: u32,
    pub transcription_sample_rate: u32,
    /// 16 kHz mono samples for whisper.
    pub transcription_samples: Vec<f32>,
    /// Optional `(system, mic)` legs at `record_sample_rate` for diarization.
    pub dual_track: Option<(Vec<f32>, Vec<f32>)>,
}

impl RecordingResult {
    /// Write the accumulated 16 kHz mono transcription stream to a WAV at
    /// `path` (whisper-ready). Pure file I/O; safe to call anywhere.
    pub fn write_transcription_wav(&self, path: &Path) -> Result<()> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.transcription_sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec)?;
        for s in f32_to_i16(&self.transcription_samples) {
            w.write_sample(s)?;
        }
        w.finalize()?;
        Ok(())
    }
}

/// Helper used by the live path (and tests) to build a `hound`-backed recorder.
pub fn create_wav_recorder(
    config: RecorderConfig,
    output_path: PathBuf,
) -> Result<Recorder<HoundWavSink>> {
    let sink = HoundWavSink::create(&output_path, config.record_sample_rate)?;
    Ok(Recorder::new(config, output_path, sink))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioChunk, Source, StreamFormat};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// In-memory sink capturing every sample written, so accounting/state tests
    /// need no filesystem and can assert exact contents.
    #[derive(Clone, Default)]
    struct FakeSink {
        samples: Rc<RefCell<Vec<i16>>>,
        finalized: Rc<RefCell<bool>>,
    }

    impl WavSink for FakeSink {
        fn write_samples(&mut self, samples: &[i16]) -> Result<usize> {
            self.samples.borrow_mut().extend_from_slice(samples);
            Ok(samples.len())
        }
        fn finalize(self: Box<Self>) -> Result<()> {
            *self.finalized.borrow_mut() = true;
            Ok(())
        }
    }

    fn recorder_with(config: RecorderConfig) -> (Recorder<FakeSink>, FakeSink) {
        let sink = FakeSink::default();
        let probe = sink.clone();
        let r = Recorder::new(config, PathBuf::from("/tmp/test.wav"), sink);
        (r, probe)
    }

    fn mono_chunk(source: Source, rate: u32, samples: Vec<f32>) -> AudioChunk {
        AudioChunk::new(source, StreamFormat::new(rate, 1), samples)
    }

    // ---- state machine ----------------------------------------------------

    #[test]
    fn fresh_recorder_is_idle() {
        let (r, _) = recorder_with(RecorderConfig::default());
        assert_eq!(r.state(), RecorderState::Idle);
    }

    #[test]
    fn legal_lifecycle_transitions() {
        let (mut r, _) = recorder_with(RecorderConfig::default());
        assert!(r.start().is_ok());
        assert_eq!(r.state(), RecorderState::Recording);
        assert!(r.pause().is_ok());
        assert_eq!(r.state(), RecorderState::Paused);
        assert!(r.resume().is_ok());
        assert_eq!(r.state(), RecorderState::Recording);
        assert!(r.stop().is_ok());
        assert_eq!(r.state(), RecorderState::Stopped);
    }

    #[test]
    fn illegal_transitions_error_not_panic() {
        let (mut r, _) = recorder_with(RecorderConfig::default());
        assert!(r.pause().is_err()); // pause from Idle
        assert!(r.resume().is_err()); // resume from Idle
        assert!(r.stop().is_err()); // stop from Idle
        r.start().unwrap();
        assert!(r.start().is_err()); // double start
        assert!(r.resume().is_err()); // resume while recording
        r.stop().unwrap();
        assert!(r.start().is_err()); // start after stop (terminal)
        assert!(r.pause().is_err());
    }

    // ---- feed + accounting ------------------------------------------------

    #[test]
    fn feed_writes_frames_and_advances_duration() {
        let cfg = RecorderConfig {
            record_sample_rate: 16_000,
            keep_dual_track: false,
            ..Default::default()
        };
        let (mut r, probe) = recorder_with(cfg);
        r.start().unwrap();
        // 16_000 mono frames at 16k = exactly 1s.
        let chunk = mono_chunk(Source::Microphone, 16_000, vec![0.25; 16_000]);
        let n = r.feed(None, Some(&chunk)).unwrap();
        assert_eq!(n, 16_000);
        assert_eq!(probe.samples.borrow().len(), 16_000);
        assert_eq!(r.duration_seconds(), 1);
        assert_eq!(r.duration_ms(), 1000);
    }

    #[test]
    fn feed_while_paused_is_noop_no_gap_corruption() {
        let cfg = RecorderConfig {
            record_sample_rate: 16_000,
            ..Default::default()
        };
        let (mut r, probe) = recorder_with(cfg);
        r.start().unwrap();
        let chunk = mono_chunk(Source::Microphone, 16_000, vec![0.5; 8_000]); // 0.5s
        r.feed(None, Some(&chunk)).unwrap();
        let frames_before = probe.samples.borrow().len();
        let dur_before = r.duration_ms();

        r.pause().unwrap();
        // Late callbacks arriving during pause must be ignored.
        assert_eq!(r.feed(None, Some(&chunk)).unwrap(), 0);
        assert_eq!(r.feed(None, Some(&chunk)).unwrap(), 0);
        assert_eq!(probe.samples.borrow().len(), frames_before);
        assert_eq!(r.duration_ms(), dur_before); // duration frozen across pause

        r.resume().unwrap();
        r.feed(None, Some(&chunk)).unwrap(); // another 0.5s
        // Total written = first 0.5s + post-resume 0.5s = 1.0s, NO paused gap.
        assert_eq!(probe.samples.borrow().len(), 16_000);
        assert_eq!(r.duration_ms(), 1000);
    }

    #[test]
    fn pause_does_not_insert_silence_into_file() {
        // The bytes written before and after a pause must be contiguous real
        // audio — no zero-filled paused region spliced in.
        let cfg = RecorderConfig {
            record_sample_rate: 16_000,
            ..Default::default()
        };
        let (mut r, probe) = recorder_with(cfg);
        r.start().unwrap();
        r.feed(None, Some(&mono_chunk(Source::Microphone, 16_000, vec![1.0; 4])))
            .unwrap();
        r.pause().unwrap();
        r.resume().unwrap();
        r.feed(None, Some(&mono_chunk(Source::Microphone, 16_000, vec![1.0; 4])))
            .unwrap();
        let written = probe.samples.borrow();
        // All 8 samples are full-scale; if a paused gap had been inserted we'd
        // see interspersed zeros.
        assert_eq!(written.len(), 8);
        assert!(written.iter().all(|&s| s == i16::MAX), "got {written:?}");
    }

    #[test]
    fn stop_finalizes_sink_and_returns_result() {
        let cfg = RecorderConfig {
            record_sample_rate: 16_000,
            ..Default::default()
        };
        let (mut r, probe) = recorder_with(cfg);
        r.start().unwrap();
        r.feed(None, Some(&mono_chunk(Source::Microphone, 16_000, vec![0.1; 16_000])))
            .unwrap();
        let res = r.stop().unwrap();
        assert!(*probe.finalized.borrow());
        assert_eq!(res.duration_ms, 1000);
        assert_eq!(res.transcription_sample_rate, 16_000);
        // Transcription stream is the 16k path — same length here since record
        // rate already equals 16k.
        assert_eq!(res.transcription_samples.len(), 16_000);
        assert_eq!(res.wav_path, PathBuf::from("/tmp/test.wav"));
    }

    #[test]
    fn dual_track_accumulates_when_enabled() {
        let cfg = RecorderConfig {
            record_sample_rate: 16_000,
            keep_dual_track: true,
            ..Default::default()
        };
        let (mut r, _) = recorder_with(cfg);
        r.start().unwrap();
        let sys = mono_chunk(Source::System, 16_000, vec![0.2; 100]);
        let mic = mono_chunk(Source::Microphone, 16_000, vec![0.1; 100]);
        r.feed(Some(&sys), Some(&mic)).unwrap();
        let res = r.stop().unwrap();
        let (s, m) = res.dual_track.expect("dual track present");
        assert_eq!(s.len(), 100);
        assert_eq!(m.len(), 100);
        assert!((s[0] - 0.2).abs() < 1e-6);
        assert!((m[0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn duration_excludes_resample_growth() {
        // Record at 48k but feed a 16k mic chunk: it upsamples 3x, so frames
        // written reflect the 48k timeline (duration stays = source duration).
        let cfg = RecorderConfig {
            record_sample_rate: 48_000,
            ..Default::default()
        };
        let (mut r, _) = recorder_with(cfg);
        r.start().unwrap();
        // 16_000 frames of 16k mic = 1.0s of audio.
        let chunk = mono_chunk(Source::Microphone, 16_000, vec![0.3; 16_000]);
        r.feed(None, Some(&chunk)).unwrap();
        // Upsampled to 48k → ~48_000 frames → ~1.0s. Allow a 1-frame rounding.
        let ms = r.duration_ms();
        assert!((999..=1001).contains(&ms), "duration {ms}ms not ~1000");
    }

    #[test]
    fn record_at_48k_produces_16k_transcription_stream() {
        let cfg = RecorderConfig {
            record_sample_rate: 48_000,
            ..Default::default()
        };
        let (mut r, _) = recorder_with(cfg);
        r.start().unwrap();
        let chunk = mono_chunk(Source::Microphone, 48_000, vec![0.3; 48_000]); // 1s
        r.feed(None, Some(&chunk)).unwrap();
        let res = r.stop().unwrap();
        // ~1s at 16k mono ≈ 16_000 samples (±1 for rounding).
        let n = res.transcription_samples.len();
        assert!((15_999..=16_001).contains(&n), "transcription len {n}");
    }
}
