//! Microphone capture via [`cpal`].
//!
//! `cpal` gives us a cross-platform input stream. The device callback fires on
//! an audio thread with whatever sample *format* the device prefers (`i16`,
//! `u16` or `f32`); we normalise every variant to interleaved `f32` in
//! `[-1.0, 1.0]` and forward [`AudioChunk`]s over a channel to the mixer.
//!
//! ## Testability boundary
//!
//! Opening a device and running a live stream needs real hardware, so those
//! calls are isolated in [`MicrophoneCapture::start`]. Everything that can be
//! pure *is* pure and unit-tested here:
//! - sample-format normalisation ([`i16_to_f32`], [`u16_to_f32`]),
//! - input-config selection ([`choose_input_config`]).
//!
//! The live-capture path is marked clearly and can only be verified on a
//! machine with a microphone.

use std::sync::mpsc::Sender;

// Some of these are only referenced from the `#[cfg(not(test))]` hardware path
// (e.g. `Source`, several `AudioError` variants), so in a `cfg(test)` build the
// import set is a superset of what's used. Allow that rather than fragmenting
// the imports across cfg blocks.
#[allow(unused_imports)]
use super::{AudioChunk, AudioError, Result, Source, StreamFormat};

#[cfg(not(test))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Normalise a slice of `i16` PCM to `f32` in `[-1.0, 1.0]`.
pub fn i16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples
        .iter()
        .map(|&s| {
            if s < 0 {
                s as f32 / -(i16::MIN as f32)
            } else {
                s as f32 / i16::MAX as f32
            }
        })
        .collect()
}

/// Normalise a slice of `u16` PCM (unsigned, mid-point 32768) to `f32`.
pub fn u16_to_f32(samples: &[u16]) -> Vec<f32> {
    samples
        .iter()
        .map(|&s| (s as f32 - 32_768.0) / 32_768.0)
        .collect()
}

/// A user's choice of microphone. `None` device name means "system default".
#[derive(Debug, Clone, Default)]
pub struct MicSelection {
    /// Exact device name as reported by the host; `None` → default input.
    pub device_name: Option<String>,
    /// Preferred capture rate (Hz). The actual rate may differ; the mixer
    /// resamples, so this is only a hint. `None` → device default.
    pub preferred_rate: Option<u32>,
}

/// Plan describing how we'll open the input device. Pulled out of the live
/// `cpal` types so the *selection logic* is unit-testable without hardware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPlan {
    pub sample_rate: u32,
    pub channels: u16,
}

impl InputPlan {
    pub fn format(&self) -> StreamFormat {
        StreamFormat::new(self.sample_rate, self.channels)
    }
}

/// Pick an [`InputPlan`] from the device's supported range. Prefers the
/// caller's `preferred_rate` when it falls inside `[min, max]`, otherwise the
/// device default. This is the pure half of config negotiation.
///
/// `supported` is `(min_rate, max_rate, default_rate, channels)` as cpal would
/// report it.
pub fn choose_input_config(
    preferred_rate: Option<u32>,
    supported: (u32, u32, u32, u16),
) -> InputPlan {
    let (min, max, default, channels) = supported;
    let rate = match preferred_rate {
        Some(p) if p >= min && p <= max => p,
        _ => default,
    };
    InputPlan {
        sample_rate: rate,
        channels: channels.max(1),
    }
}

/// Live microphone capture handle. Holds the running `cpal` stream; dropping it
/// (or calling [`stop`](MicrophoneCapture::stop)) tears the stream down.
pub struct MicrophoneCapture {
    #[cfg(not(test))]
    stream: cpal::Stream,
    format: StreamFormat,
}

impl MicrophoneCapture {
    /// Format the stream is delivering (post-negotiation). Useful for seeding a
    /// [`super::mixer::DriftCompensator`] with the right nominal rate.
    pub fn format(&self) -> StreamFormat {
        self.format
    }

    /// Open the selected microphone and start streaming `f32` [`AudioChunk`]s to
    /// `sink`. Returns once the stream is running.
    ///
    /// HARDWARE PATH — needs a real input device; verify on target hardware.
    #[cfg(not(test))]
    pub fn start(sel: &MicSelection, sink: Sender<AudioChunk>) -> Result<Self> {
        let host = cpal::default_host();

        let device = match &sel.device_name {
            Some(name) => host
                .input_devices()
                .map_err(|e| AudioError::Stream(e.to_string()))?
                .find(|d| d.name().map(|n| &n == name).unwrap_or(false))
                .ok_or_else(|| AudioError::DeviceNotFound(name.clone()))?,
            None => host
                .default_input_device()
                .ok_or_else(|| AudioError::DeviceNotFound("default input".into()))?,
        };

        let supported = device
            .default_input_config()
            .map_err(|e| AudioError::UnsupportedConfig(e.to_string()))?;
        let sample_format = supported.sample_format();

        // Negotiate the rate/channels via the pure planner, then materialise a
        // concrete cpal config.
        let dev_default = supported.config();
        let ranges = device
            .supported_input_configs()
            .map_err(|e| AudioError::UnsupportedConfig(e.to_string()))?;
        let (mut min_r, mut max_r) = (
            dev_default.sample_rate.0,
            dev_default.sample_rate.0,
        );
        for r in ranges {
            min_r = min_r.min(r.min_sample_rate().0);
            max_r = max_r.max(r.max_sample_rate().0);
        }
        let plan = choose_input_config(
            sel.preferred_rate,
            (min_r, max_r, dev_default.sample_rate.0, dev_default.channels),
        );
        let format = plan.format();

        let config = cpal::StreamConfig {
            channels: plan.channels,
            sample_rate: cpal::SampleRate(plan.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let err_fn = |e| eprintln!("microphone stream error: {e}");

        // One closure per sample format; each normalises to f32 then forwards.
        macro_rules! build {
            ($t:ty, $conv:expr) => {{
                let sink = sink.clone();
                device
                    .build_input_stream(
                        &config,
                        move |data: &[$t], _: &cpal::InputCallbackInfo| {
                            let samples: Vec<f32> = $conv(data);
                            // A full channel is fine; drop on disconnect.
                            let _ = sink.send(AudioChunk::new(
                                Source::Microphone,
                                format,
                                samples,
                            ));
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| AudioError::Stream(e.to_string()))?
            }};
        }

        let stream = match sample_format {
            cpal::SampleFormat::F32 => build!(f32, |d: &[f32]| d.to_vec()),
            cpal::SampleFormat::I16 => build!(i16, |d: &[i16]| i16_to_f32(d)),
            cpal::SampleFormat::U16 => build!(u16, |d: &[u16]| u16_to_f32(d)),
            other => {
                return Err(AudioError::UnsupportedConfig(format!(
                    "unsupported sample format {other:?}"
                )))
            }
        };

        stream
            .play()
            .map_err(|e| AudioError::Stream(e.to_string()))?;

        Ok(Self { stream, format })
    }

    /// Test build: no real device. Returns a capture whose `format` echoes the
    /// negotiated plan so orchestration logic can be exercised without hardware.
    #[cfg(test)]
    pub fn start(sel: &MicSelection, _sink: Sender<AudioChunk>) -> Result<Self> {
        let plan = choose_input_config(sel.preferred_rate, (8_000, 192_000, 48_000, 1));
        Ok(Self {
            format: plan.format(),
        })
    }

    /// Stop and release the device.
    pub fn stop(self) -> Result<()> {
        #[cfg(not(test))]
        {
            self.stream
                .pause()
                .map_err(|e| AudioError::Stream(e.to_string()))?;
            drop(self.stream);
        }
        Ok(())
    }
}

/// Enumerate available input device names. HARDWARE PATH; on test builds returns
/// an empty list so callers can be exercised deterministically.
#[cfg(not(test))]
pub fn list_input_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    for d in host
        .input_devices()
        .map_err(|e| AudioError::Stream(e.to_string()))?
    {
        if let Ok(n) = d.name() {
            names.push(n);
        }
    }
    Ok(names)
}

#[cfg(test)]
pub fn list_input_devices() -> Result<Vec<String>> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i16_full_scale_normalises_to_unit() {
        assert!((i16_to_f32(&[i16::MAX])[0] - 1.0).abs() < 1e-6);
        assert!((i16_to_f32(&[i16::MIN])[0] - -1.0).abs() < 1e-6);
        assert_eq!(i16_to_f32(&[0])[0], 0.0);
    }

    #[test]
    fn u16_midpoint_is_zero() {
        assert!((u16_to_f32(&[32_768])[0] - 0.0).abs() < 1e-6);
        assert!((u16_to_f32(&[65_535])[0] - 0.999969).abs() < 1e-3);
        assert!((u16_to_f32(&[0])[0] - -1.0).abs() < 1e-6);
    }

    #[test]
    fn choose_config_honours_preferred_when_in_range() {
        let plan = choose_input_config(Some(16_000), (8_000, 48_000, 44_100, 2));
        assert_eq!(plan.sample_rate, 16_000);
        assert_eq!(plan.channels, 2);
    }

    #[test]
    fn choose_config_falls_back_to_default_when_out_of_range() {
        let plan = choose_input_config(Some(192_000), (8_000, 48_000, 44_100, 1));
        assert_eq!(plan.sample_rate, 44_100);
    }

    #[test]
    fn choose_config_default_when_no_preference() {
        let plan = choose_input_config(None, (8_000, 48_000, 48_000, 1));
        assert_eq!(plan.sample_rate, 48_000);
    }

    #[test]
    fn choose_config_clamps_zero_channels_to_one() {
        let plan = choose_input_config(None, (8_000, 48_000, 48_000, 0));
        assert_eq!(plan.channels, 1);
    }

    #[test]
    fn start_in_test_build_reports_negotiated_format() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let cap = MicrophoneCapture::start(
            &MicSelection {
                device_name: None,
                preferred_rate: Some(16_000),
            },
            tx,
        )
        .unwrap();
        assert_eq!(cap.format().sample_rate, 16_000);
        cap.stop().unwrap();
    }
}
