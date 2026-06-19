//! Audio capture module.
//!
//! v1.0 (per docs/PRD.md §4.4): **microphone** (`cpal`) + **system audio**
//! (WASAPI loopback, Windows-only) → **mixer** (resample both legs to a common
//! rate, compensate for clock drift / buffer under-overrun, downmix to mono) →
//! **recorder** (orchestrates start / pause / resume / stop, writes WAV via
//! `hound`, tracks duration).
//!
//! Two output paths come out of the mixer:
//! 1. The **transcription path** — a single 16 kHz mono stream fed to whisper.
//! 2. An optional **dual-track** archive (system + mic kept separate) that the
//!    diarization step can use as a hint.
//!
//! ## What is pure and what is hardware-bound
//!
//! The DSP in [`mixer`] (resampling, drift compensation, mono downmix, sample
//! conversion) is **pure** and exhaustively unit-tested in this crate. The
//! capture paths ([`microphone`], [`system_audio`]) touch real devices and the
//! OS audio stack; they are structured so the hardware calls are isolated and
//! everything around them is testable, but the capture itself can only be
//! verified on a machine with the right devices (Windows, for loopback).

pub mod microphone;
pub mod mixer;
pub mod recorder;
pub mod system_audio;

use std::fmt;

/// Sample rate, in Hz, of the 16 kHz mono stream whisper expects (PRD §4.4).
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Default sample rate we resample everything to for the *recording* (archive)
/// path when no explicit target is given. 48 kHz is the WASAPI shared-mode
/// default on Windows, so it minimises resampling on the system-audio leg.
pub const DEFAULT_RECORD_SAMPLE_RATE: u32 = 48_000;

/// Errors surfaced by the audio subsystem.
///
/// `Unsupported` is what the non-Windows [`system_audio`] stub returns so the
/// crate compiles and links everywhere while making the v1.1 boundary explicit.
#[derive(Debug)]
pub enum AudioError {
    /// No input/output device matched the request.
    DeviceNotFound(String),
    /// The device or backend rejected a configuration (rate, channels, format).
    UnsupportedConfig(String),
    /// The underlying capture/render stream failed to build or run.
    Stream(String),
    /// WAV (or other file) I/O failed.
    Io(String),
    /// A control transition was invalid (e.g. resume while stopped).
    InvalidState(String),
    /// The requested capability is not available on this OS in v1.0.
    Unsupported(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::DeviceNotFound(d) => write!(f, "audio device not found: {d}"),
            AudioError::UnsupportedConfig(c) => write!(f, "unsupported audio config: {c}"),
            AudioError::Stream(s) => write!(f, "audio stream error: {s}"),
            AudioError::Io(s) => write!(f, "audio io error: {s}"),
            AudioError::InvalidState(s) => write!(f, "invalid recorder state: {s}"),
            AudioError::Unsupported(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for AudioError {}

impl From<std::io::Error> for AudioError {
    fn from(e: std::io::Error) -> Self {
        AudioError::Io(e.to_string())
    }
}

impl From<hound::Error> for AudioError {
    fn from(e: hound::Error) -> Self {
        AudioError::Io(e.to_string())
    }
}

/// Result alias for the audio subsystem.
pub type Result<T> = std::result::Result<T, AudioError>;

/// Which leg of the mix a captured buffer belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// Local microphone (via `cpal`).
    Microphone,
    /// System audio loopback (WASAPI on Windows).
    System,
}

/// Describes the interleaved PCM format of a captured chunk. All capture
/// backends normalise to `f32` samples in `[-1.0, 1.0]` before handing buffers
/// to the mixer, so only the rate and channel count vary here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    /// Samples per second per channel.
    pub sample_rate: u32,
    /// Number of interleaved channels (1 = mono, 2 = stereo, ...).
    pub channels: u16,
}

impl StreamFormat {
    pub const fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
        }
    }

    /// The canonical whisper input: 16 kHz mono.
    pub const fn whisper() -> Self {
        Self::new(WHISPER_SAMPLE_RATE, 1)
    }

    /// Number of full frames in an interleaved buffer of `len` samples.
    /// A *frame* is one sample per channel.
    pub fn frames(self, interleaved_len: usize) -> usize {
        if self.channels == 0 {
            0
        } else {
            interleaved_len / self.channels as usize
        }
    }
}

/// A chunk of captured audio in `f32` interleaved PCM, tagged with its source
/// and format. This is the unit the mixer consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioChunk {
    pub source: Source,
    pub format: StreamFormat,
    /// Interleaved `f32` samples in `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
}

impl AudioChunk {
    pub fn new(source: Source, format: StreamFormat, samples: Vec<f32>) -> Self {
        Self {
            source,
            format,
            samples,
        }
    }

    /// Number of frames (samples-per-channel) in this chunk.
    pub fn frames(&self) -> usize {
        self.format.frames(self.samples.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_format_is_16k_mono() {
        let f = StreamFormat::whisper();
        assert_eq!(f.sample_rate, 16_000);
        assert_eq!(f.channels, 1);
    }

    #[test]
    fn frames_counts_per_channel() {
        let stereo = StreamFormat::new(48_000, 2);
        assert_eq!(stereo.frames(8), 4);
        // Trailing partial frame is truncated, never panics.
        assert_eq!(stereo.frames(7), 3);
        let mono = StreamFormat::new(16_000, 1);
        assert_eq!(mono.frames(7), 7);
    }

    #[test]
    fn zero_channels_is_zero_frames_not_panic() {
        let weird = StreamFormat::new(48_000, 0);
        assert_eq!(weird.frames(100), 0);
    }

    #[test]
    fn chunk_reports_frames() {
        let c = AudioChunk::new(
            Source::Microphone,
            StreamFormat::new(48_000, 2),
            vec![0.0; 10],
        );
        assert_eq!(c.frames(), 5);
    }

    #[test]
    fn audio_error_unsupported_displays_message_verbatim() {
        let e = AudioError::Unsupported("system audio loopback unsupported (v1.1)".into());
        assert_eq!(e.to_string(), "system audio loopback unsupported (v1.1)");
    }
}
