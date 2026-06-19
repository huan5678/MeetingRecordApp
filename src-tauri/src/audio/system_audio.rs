//! System-audio loopback capture.
//!
//! v1.0 captures system audio on **Windows only** via WASAPI loopback (the
//! `wasapi` crate). On every other OS the same public API is present but every
//! entry point returns [`AudioError::Unsupported`] with a clear "(v1.1)"
//! message, so the crate compiles and links on macOS/Linux while making the
//! platform boundary explicit (PRD §4.4, §6.2).
//!
//! WASAPI loopback works by opening the *render* (output) endpoint in capture
//! mode: you read back exactly what is being played to the speakers. The device
//! delivers `f32` (or `i16`) frames at the endpoint's mix format — typically
//! 48 kHz stereo — which we normalise to interleaved `f32` and forward as
//! [`AudioChunk`]s, identical in shape to the microphone path so the mixer
//! treats both legs uniformly.
//!
//! ## Testability boundary
//!
//! Live loopback needs a Windows audio endpoint, so the capture loop is gated.
//! The cross-platform pieces — byte→`f32` deinterleaving and the
//! unsupported-platform contract — are pure and tested here.

use std::sync::mpsc::Sender;

// The `platform` submodules below pull these in via `use super::*`; which subset
// is actually used depends on the target OS (the non-Windows stub never touches
// `Source`/`StreamFormat`). Allow the superset rather than cfg-splitting.
#[allow(unused_imports)]
use super::{AudioChunk, AudioError, Result, Source, StreamFormat};

/// User's choice of loopback target. `None` device name → default render
/// endpoint (the speakers the OS is currently playing to).
#[derive(Debug, Clone, Default)]
pub struct LoopbackSelection {
    pub device_name: Option<String>,
}

/// Reinterpret a raw little-endian byte buffer of 32-bit floats (as WASAPI
/// hands back) into a `Vec<f32>`. Pure and platform-independent so it can be
/// tested everywhere. Trailing bytes that don't form a full `f32` are ignored.
pub fn bytes_to_f32_le(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// Reinterpret a raw little-endian byte buffer of signed 16-bit PCM into
/// normalised `f32` in `[-1.0, 1.0]`.
pub fn bytes_i16_to_f32_le(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|b| {
            let s = i16::from_le_bytes([b[0], b[1]]);
            if s < 0 {
                s as f32 / -(i16::MIN as f32)
            } else {
                s as f32 / i16::MAX as f32
            }
        })
        .collect()
}

// ============================================================================
// Windows implementation (WASAPI loopback)
// ============================================================================
#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;

    use wasapi::{
        get_default_device, initialize_mta, Direction, SampleType, ShareMode, WaveFormat,
    };

    /// Running loopback capture. The capture loop lives on its own thread; the
    /// handle owns a stop-flag and the join handle so [`stop`] is clean.
    pub struct SystemAudioCapture {
        stop: Arc<AtomicBool>,
        join: Option<JoinHandle<()>>,
        format: StreamFormat,
    }

    impl SystemAudioCapture {
        pub fn format(&self) -> StreamFormat {
            self.format
        }

        /// Open the default render endpoint in loopback mode and stream
        /// `f32` [`AudioChunk`]s to `sink`.
        ///
        /// HARDWARE PATH — needs a Windows audio endpoint; verify on Windows.
        pub fn start(_sel: &LoopbackSelection, sink: Sender<AudioChunk>) -> Result<Self> {
            // COM must be initialised on the capture thread, so we do all WASAPI
            // work inside the spawned thread and report the negotiated format
            // back through a one-shot channel.
            let stop = Arc::new(AtomicBool::new(false));
            let stop_thread = stop.clone();
            let (fmt_tx, fmt_rx) = std::sync::mpsc::channel::<Result<StreamFormat>>();

            let join = std::thread::Builder::new()
                .name("wasapi-loopback".into())
                .spawn(move || {
                    let run = || -> Result<()> {
                        initialize_mta()
                            .ok()
                            .map_err(|e| AudioError::Stream(format!("COM init: {e}")))?;

                        // The render device, opened for loopback capture.
                        let device = get_default_device(&Direction::Render)
                            .map_err(|e| AudioError::DeviceNotFound(e.to_string()))?;
                        let mut audio_client = device
                            .get_iaudioclient()
                            .map_err(|e| AudioError::Stream(e.to_string()))?;

                        // Match the endpoint's own mix format to avoid an extra
                        // resample inside WASAPI; the app-side mixer handles
                        // rate conversion to the common target.
                        let mix = audio_client
                            .get_mixformat()
                            .map_err(|e| AudioError::UnsupportedConfig(e.to_string()))?;
                        let channels = mix.get_nchannels();
                        let sample_rate = mix.get_samplespersec();
                        let bits = mix.get_bitspersample();
                        let sample_type = mix.get_subformat().unwrap_or(SampleType::Float);

                        let desired = WaveFormat::new(
                            bits as usize,
                            bits as usize,
                            &sample_type,
                            sample_rate as usize,
                            channels as usize,
                            None,
                        );

                        let (_def_period, min_period) = audio_client
                            .get_periods()
                            .map_err(|e| AudioError::Stream(e.to_string()))?;

                        audio_client
                            .initialize_client(
                                &desired,
                                min_period,
                                &Direction::Capture, // loopback = capture on a render device
                                &ShareMode::Shared,
                                true, // loopback
                            )
                            .map_err(|e| AudioError::Stream(e.to_string()))?;

                        let format =
                            StreamFormat::new(sample_rate, channels);
                        // Report the format to the caller before we start looping.
                        let _ = fmt_tx.send(Ok(format));

                        let event = audio_client
                            .set_get_eventhandle()
                            .map_err(|e| AudioError::Stream(e.to_string()))?;
                        let capture = audio_client
                            .get_audiocaptureclient()
                            .map_err(|e| AudioError::Stream(e.to_string()))?;
                        audio_client
                            .start_stream()
                            .map_err(|e| AudioError::Stream(e.to_string()))?;

                        let block_align = desired.get_blockalign() as usize;
                        let mut raw: Vec<u8> = Vec::with_capacity(block_align * 1024);

                        while !stop_thread.load(Ordering::Relaxed) {
                            // Wait (with timeout) for the device to signal data.
                            if event.wait_for_event(200).is_err() {
                                continue; // timeout → re-check stop flag
                            }
                            raw.clear();
                            if capture
                                .read_from_device_to_deque(&mut raw)
                                .is_err()
                            {
                                continue;
                            }
                            if raw.is_empty() {
                                continue;
                            }
                            let samples = match sample_type {
                                SampleType::Float => bytes_to_f32_le(&raw),
                                SampleType::Int => bytes_i16_to_f32_le(&raw),
                            };
                            if sink
                                .send(AudioChunk::new(Source::System, format, samples))
                                .is_err()
                            {
                                break; // consumer gone
                            }
                        }
                        let _ = audio_client.stop_stream();
                        Ok(())
                    };

                    if let Err(e) = run() {
                        // If we failed before reporting a format, surface it.
                        let _ = fmt_tx.send(Err(e));
                    }
                })
                .map_err(|e| AudioError::Stream(e.to_string()))?;

            // Block until the thread reports its negotiated format (or an error).
            let format = fmt_rx
                .recv()
                .map_err(|_| AudioError::Stream("loopback thread exited early".into()))??;

            Ok(Self {
                stop,
                join: Some(join),
                format,
            })
        }

        /// Signal the capture loop to stop and join it.
        pub fn stop(mut self) -> Result<()> {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
            Ok(())
        }
    }

    impl Drop for SystemAudioCapture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    pub fn list_loopback_devices() -> Result<Vec<String>> {
        // Loopback targets are render endpoints; enumerate them by name.
        use wasapi::{get_default_device, Direction};
        let mut names = Vec::new();
        if let Ok(dev) = get_default_device(&Direction::Render) {
            if let Ok(n) = dev.get_friendlyname() {
                names.push(n);
            }
        }
        Ok(names)
    }
}

// ============================================================================
// Non-Windows stub — compiles and links, returns a clear v1.1 error
// ============================================================================
#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    /// Stand-in handle so the public type exists on every platform. The
    /// `Infallible` field makes it *uninhabited*: it can never be constructed,
    /// which is correct because [`start`] always errors off Windows. The
    /// accessor bodies are therefore unreachable.
    ///
    /// `Debug` is derived (free: `Infallible: Debug`) so the off-Windows tests
    /// can call `start(..).unwrap_err()` — `Result::unwrap_err` requires the
    /// `Ok` type to be `Debug`.
    #[derive(Debug)]
    pub struct SystemAudioCapture {
        #[allow(dead_code)]
        never: std::convert::Infallible,
    }

    impl SystemAudioCapture {
        pub fn format(&self) -> StreamFormat {
            // Unreachable: no value of this (uninhabited) type can exist, so
            // this method can never actually be called.
            unreachable!("SystemAudioCapture is uninhabited off Windows")
        }

        /// Always unsupported off Windows in v1.0.
        pub fn start(_sel: &LoopbackSelection, _sink: Sender<AudioChunk>) -> Result<Self> {
            Err(unsupported())
        }

        pub fn stop(self) -> Result<()> {
            unreachable!("SystemAudioCapture is uninhabited off Windows")
        }
    }

    pub fn list_loopback_devices() -> Result<Vec<String>> {
        Err(unsupported())
    }

    fn unsupported() -> AudioError {
        AudioError::Unsupported(
            "system audio loopback is unsupported on this platform in v1.0 \
             (Windows-only); planned for v1.1"
                .into(),
        )
    }
}

pub use platform::{list_loopback_devices, SystemAudioCapture};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_to_f32_round_trips_known_values() {
        let mut bytes = Vec::new();
        for v in [0.0f32, 1.0, -1.0, 0.5] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let out = bytes_to_f32_le(&bytes);
        assert_eq!(out, vec![0.0, 1.0, -1.0, 0.5]);
    }

    #[test]
    fn bytes_to_f32_ignores_trailing_partial() {
        // 5 bytes = one full f32 + 1 stray byte.
        let mut bytes = 0.25f32.to_le_bytes().to_vec();
        bytes.push(0xAB);
        let out = bytes_to_f32_le(&bytes);
        assert_eq!(out, vec![0.25]);
    }

    #[test]
    fn bytes_i16_normalises() {
        let mut bytes = Vec::new();
        for v in [0i16, i16::MAX, i16::MIN] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let out = bytes_i16_to_f32_le(&bytes);
        assert_eq!(out[0], 0.0);
        assert!((out[1] - 1.0).abs() < 1e-6);
        assert!((out[2] - -1.0).abs() < 1e-6);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn loopback_is_unsupported_off_windows_with_v1_1_message() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let err = SystemAudioCapture::start(&LoopbackSelection::default(), tx).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("v1.1"), "missing v1.1 marker: {msg}");
        assert!(msg.contains("unsupported"), "missing 'unsupported': {msg}");

        let list_err = list_loopback_devices().unwrap_err();
        assert!(list_err.to_string().contains("v1.1"));
    }
}
