//! Live capture session — the Phase 2 wiring that turns the (pure, tested)
//! [`recorder`](super::recorder) + [`mixer`](super::mixer) + capture backends
//! into a running recording driven by a background thread.
//!
//! ## Why a dedicated thread
//!
//! `cpal::Stream` (microphone) is `!Send`, so it cannot live inside Tauri's
//! managed state (which must be `Send + Sync`). The solution: spawn ONE
//! `audio-capture` thread that *creates and owns* the capture streams locally
//! and never moves them. The only thing handed back to the command layer is a
//! [`LiveHandle`] built entirely from `Send + Sync` primitives (atomics, a
//! mutex-guarded result slot, a join handle), which is safe to store in
//! `AppState`.
//!
//! ```text
//! cpal mic  ─┐                              ┌─ mic SampleQueue ─┐
//!            ├─ Sender<AudioChunk> ─ recv ──┤                   ├─ Recorder.feed → WAV
//! WASAPI sys ┘   (audio-capture thread)     └─ sys SampleQueue ─┘     (mixed mono)
//! ```
//!
//! ## Mixing strategy (and its one runtime caveat)
//!
//! Each incoming device chunk is downmixed + drift-corrected-resampled to the
//! record rate ([`mixer::prepare_leg`]) and pushed into its leg's
//! [`SampleQueue`]. The **microphone is the master clock**: each drain pulls all
//! available mic frames and an equal count from the system queue (silence-padded
//! if the system leg is short). This deliberately sidesteps a real WASAPI
//! loopback quirk — during pure silence the endpoint may deliver *no* packets —
//! which a naive `min(both queues)` drain would stall on. The recorder's own
//! drift compensators are left neutral; all drift correction happens here on
//! ingest. **This cross-stream alignment is the PRD §7.5 #1 risk and must be
//! validated on Windows (Phase 0 spike); it compiles and is structured here but
//! cannot be exercised on the dev Mac.**

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::microphone::{MicSelection, MicrophoneCapture};
use super::mixer::{self, DriftCompensator, SampleQueue};
use super::recorder::{
    create_wav_recorder, HoundWavSink, Recorder, RecorderConfig, RecorderState, RecordingResult,
};
use super::system_audio::{LoopbackSelection, SystemAudioCapture};
use super::{AudioChunk, AudioError, Result, Source, StreamFormat};

// Control-state codes stored in the shared `AtomicU8`.
const ST_RECORDING: u8 = 0;
const ST_PAUSED: u8 = 1;
const ST_STOPPING: u8 = 2;

/// How long the consumer blocks waiting for a chunk before re-checking the
/// control flag. Small enough that pause/stop feel instant.
const POLL: Duration = Duration::from_millis(50);

/// Everything needed to open a recording session.
pub struct LiveConfig {
    pub mic: MicSelection,
    pub loopback: LoopbackSelection,
    pub recorder: RecorderConfig,
    /// Absolute path of the main recording WAV to create.
    pub wav_path: PathBuf,
}

/// State shared between the command thread (via [`LiveHandle`]) and the
/// background `audio-capture` thread. All fields are `Sync`.
struct Shared {
    /// One of `ST_RECORDING` / `ST_PAUSED` / `ST_STOPPING`.
    state: AtomicU8,
    /// Latest mic / system peak level in `[0,1]`, stored as `f32` bits, for the
    /// UI VU meter.
    mic_level: AtomicU32,
    sys_level: AtomicU32,
    /// Authoritative recorded duration (from frames written), in ms.
    duration_ms: AtomicU64,
    /// Filled once when the session stops, so [`LiveHandle::stop`] can return it.
    result: Mutex<Option<RecordingResult>>,
}

/// A handle to a running recording. `Send + Sync`, so it lives in `AppState`.
pub struct LiveHandle {
    shared: Arc<Shared>,
    join: Option<JoinHandle<()>>,
    /// Whether each leg actually started (system audio is Windows-only / may
    /// fall back to mic-only per PRD §7.5).
    pub mic_active: bool,
    pub system_active: bool,
}

impl LiveHandle {
    /// Suspend capture. Frames stop being written; duration freezes.
    pub fn pause(&self) {
        self.shared.state.store(ST_PAUSED, Ordering::SeqCst);
    }

    /// Resume after a [`pause`](Self::pause).
    pub fn resume(&self) {
        self.shared.state.store(ST_RECORDING, Ordering::SeqCst);
    }

    /// Recorded duration so far, in whole seconds (paused time excluded).
    pub fn duration_seconds(&self) -> i64 {
        (self.shared.duration_ms.load(Ordering::Relaxed) / 1000) as i64
    }

    /// Latest microphone peak level in `[0,1]`.
    pub fn mic_level(&self) -> f32 {
        f32::from_bits(self.shared.mic_level.load(Ordering::Relaxed))
    }

    /// Latest system-audio peak level in `[0,1]`.
    pub fn system_level(&self) -> f32 {
        f32::from_bits(self.shared.sys_level.load(Ordering::Relaxed))
    }

    /// Stop capture, join the thread, and return the finished recording. Consumes
    /// the handle.
    pub fn stop(mut self) -> Result<RecordingResult> {
        self.shared.state.store(ST_STOPPING, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        self.shared
            .result
            .lock()
            .map_err(|_| AudioError::InvalidState("result lock poisoned".into()))?
            .take()
            .ok_or_else(|| AudioError::InvalidState("recording produced no result".into()))
    }
}

/// Start a recording session: spawn the capture thread, start the requested
/// legs, and block until the thread confirms it is running (or reports why it
/// could not start). Returns a `Send` handle for control + status.
pub fn start_session(cfg: LiveConfig) -> Result<LiveHandle> {
    let shared = Arc::new(Shared {
        state: AtomicU8::new(ST_RECORDING),
        mic_level: AtomicU32::new(0),
        sys_level: AtomicU32::new(0),
        duration_ms: AtomicU64::new(0),
        result: Mutex::new(None),
    });
    let shared_thread = shared.clone();

    // The thread reports `(mic_active, system_active)` on success, or an error if
    // no leg could be started / the WAV could not be created.
    let (started_tx, started_rx) = mpsc::channel::<Result<(bool, bool)>>();

    let join = std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || run_session(cfg, shared_thread, started_tx))
        .map_err(|e| AudioError::Stream(e.to_string()))?;

    match started_rx.recv() {
        Ok(Ok((mic_active, system_active))) => Ok(LiveHandle {
            shared,
            join: Some(join),
            mic_active,
            system_active,
        }),
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            let _ = join.join();
            Err(AudioError::Stream(
                "capture thread exited before signalling start".into(),
            ))
        }
    }
}

/// Body of the `audio-capture` thread: start legs, create the recorder, then run
/// the ingest→mix→write loop until stopped.
fn run_session(cfg: LiveConfig, shared: Arc<Shared>, started: mpsc::Sender<Result<(bool, bool)>>) {
    let (audio_tx, audio_rx) = mpsc::channel::<AudioChunk>();

    // --- start capture legs (owned locally on this thread) ---
    let mut mic_cap: Option<MicrophoneCapture> = None;
    let mut mic_active = false;
    if cfg.recorder.capture_microphone {
        match MicrophoneCapture::start(&cfg.mic, audio_tx.clone()) {
            Ok(c) => {
                mic_cap = Some(c);
                mic_active = true;
            }
            Err(e) => eprintln!("microphone capture failed to start: {e}"),
        }
    }

    let mut sys_cap: Option<SystemAudioCapture> = None;
    let mut system_active = false;
    if cfg.recorder.capture_system {
        match SystemAudioCapture::start(&cfg.loopback, audio_tx.clone()) {
            Ok(c) => {
                sys_cap = Some(c);
                system_active = true;
            }
            // PRD §7.5 mitigation: fall back to mic-only rather than failing the
            // whole session when system audio is unavailable (e.g. off Windows).
            Err(e) => eprintln!("system-audio capture unavailable, continuing without it: {e}"),
        }
    }

    if !mic_active && !system_active {
        let _ = started.send(Err(AudioError::DeviceNotFound(
            "no audio capture leg could be started".into(),
        )));
        return;
    }

    // --- create + start the recorder ---
    let mut recorder = match create_wav_recorder(cfg.recorder.clone(), cfg.wav_path.clone()) {
        Ok(r) => r,
        Err(e) => {
            let _ = started.send(Err(e));
            return;
        }
    };
    if let Err(e) = recorder.start() {
        let _ = started.send(Err(e));
        return;
    }

    let _ = started.send(Ok((mic_active, system_active)));

    // --- consumer state ---
    let record_rate = cfg.recorder.record_sample_rate;
    let fmt = StreamFormat::new(record_rate, 1);
    let qcap = cfg.recorder.queue_capacity.max(record_rate as usize);
    let mut mic_q = SampleQueue::new(qcap);
    let mut sys_q = SampleQueue::new(qcap);
    // Ingest-side drift compensators (the recorder's internal ones stay neutral —
    // we pass it already-record-rate chunks).
    let mut mic_drift = DriftCompensator::new(record_rate, 0.1, 0.02);
    let mut sys_drift = DriftCompensator::new(record_rate, 0.1, 0.02);
    let mut mic_last: Option<Instant> = None;
    let mut sys_last: Option<Instant> = None;
    let mut last_state = ST_RECORDING;

    // Hold one Sender so `recv_timeout` never returns `Disconnected` even if a
    // capture stream dies; we exit only via the explicit stop flag.
    let _keepalive = audio_tx;

    loop {
        let recv = audio_rx.recv_timeout(POLL);

        // Apply any pause/resume/stop transition first.
        let cur = shared.state.load(Ordering::SeqCst);
        if cur != last_state {
            match cur {
                ST_PAUSED => {
                    let _ = recorder.pause();
                    mic_q.clear();
                    sys_q.clear();
                }
                ST_RECORDING if last_state == ST_PAUSED => {
                    let _ = recorder.resume();
                }
                _ => {}
            }
            last_state = cur;
        }
        if cur == ST_STOPPING {
            break;
        }

        match recv {
            Ok(chunk) => {
                if cur == ST_RECORDING {
                    match chunk.source {
                        Source::Microphone => ingest(
                            &chunk,
                            &mut mic_drift,
                            &mut mic_q,
                            &shared.mic_level,
                            &mut mic_last,
                            record_rate,
                        ),
                        Source::System => ingest(
                            &chunk,
                            &mut sys_drift,
                            &mut sys_q,
                            &shared.sys_level,
                            &mut sys_last,
                            record_rate,
                        ),
                    }
                    drain(
                        &mut recorder,
                        &mut mic_q,
                        &mut sys_q,
                        mic_active,
                        system_active,
                        fmt,
                        &shared.duration_ms,
                    );
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if cur == ST_RECORDING {
                    drain(
                        &mut recorder,
                        &mut mic_q,
                        &mut sys_q,
                        mic_active,
                        system_active,
                        fmt,
                        &shared.duration_ms,
                    );
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    // Flush anything buffered, then finalise.
    if recorder.state() == RecorderState::Recording {
        drain(
            &mut recorder,
            &mut mic_q,
            &mut sys_q,
            mic_active,
            system_active,
            fmt,
            &shared.duration_ms,
        );
    }
    if let Ok(res) = recorder.stop() {
        shared.duration_ms.store(res.duration_ms, Ordering::Relaxed);
        if let Ok(mut slot) = shared.result.lock() {
            *slot = Some(res);
        }
    }

    // Tear down the capture streams (on this thread, where they were created).
    if let Some(c) = mic_cap.take() {
        let _ = c.stop();
    }
    if let Some(c) = sys_cap.take() {
        let _ = c.stop();
    }
}

/// Downmix + drift-corrected-resample one device chunk to record-rate mono and
/// queue it; also update the leg's peak level for the UI.
fn ingest(
    chunk: &AudioChunk,
    drift: &mut DriftCompensator,
    queue: &mut SampleQueue,
    level: &AtomicU32,
    last: &mut Option<Instant>,
    record_rate: u32,
) {
    let now = Instant::now();
    let dt = last.map(|l| (now - l).as_secs_f64()).unwrap_or(0.0);
    *last = Some(now);

    drift.observe(chunk.frames(), dt);
    let mono = mixer::prepare_leg(chunk, record_rate, drift);
    let peak = mono.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    level.store(peak.to_bits(), Ordering::Relaxed);
    queue.push(&mono);
}

/// Pull aligned windows from the active leg queues and write the mixed result.
/// The mic leg is the master clock; the system leg contributes an equal frame
/// count (silence-padded by [`SampleQueue::pop`] when short).
fn drain(
    recorder: &mut Recorder<HoundWavSink>,
    mic_q: &mut SampleQueue,
    sys_q: &mut SampleQueue,
    mic_active: bool,
    system_active: bool,
    fmt: StreamFormat,
    duration: &AtomicU64,
) {
    let res = if mic_active {
        let n = mic_q.len();
        if n == 0 {
            return;
        }
        let mic_chunk = AudioChunk::new(Source::Microphone, fmt, mic_q.pop(n));
        if system_active {
            let sys_chunk = AudioChunk::new(Source::System, fmt, sys_q.pop(n));
            recorder.feed(Some(&sys_chunk), Some(&mic_chunk))
        } else {
            recorder.feed(None, Some(&mic_chunk))
        }
    } else if system_active {
        let n = sys_q.len();
        if n == 0 {
            return;
        }
        let sys_chunk = AudioChunk::new(Source::System, fmt, sys_q.pop(n));
        recorder.feed(Some(&sys_chunk), None)
    } else {
        return;
    };

    if res.is_ok() {
        duration.store(recorder.duration_ms(), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_wav(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mra_live_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("recording.wav")
    }

    /// On the dev machine (cpal mic is a test stub, no system audio) a mic-only
    /// session must start, run, and stop cleanly with a result.
    #[test]
    fn mic_only_session_starts_and_stops() {
        let wav = temp_wav("miconly");
        let cfg = LiveConfig {
            mic: MicSelection::default(),
            loopback: LoopbackSelection::default(),
            recorder: RecorderConfig {
                capture_system: false,
                capture_microphone: true,
                ..Default::default()
            },
            wav_path: wav.clone(),
        };
        let handle = start_session(cfg).expect("mic-only session should start");
        assert!(handle.mic_active);
        assert!(!handle.system_active);
        let res = handle.stop().expect("stop should yield a result");
        assert_eq!(res.wav_path, wav);
    }

    /// Requesting *only* system audio where it is unsupported (off Windows) must
    /// fail to start rather than hang or silently record nothing.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn system_only_session_fails_off_windows() {
        let wav = temp_wav("sysonly");
        let cfg = LiveConfig {
            mic: MicSelection::default(),
            loopback: LoopbackSelection::default(),
            recorder: RecorderConfig {
                capture_system: true,
                capture_microphone: false,
                ..Default::default()
            },
            wav_path: wav,
        };
        assert!(
            start_session(cfg).is_err(),
            "system-only must fail where loopback is unsupported"
        );
    }

    /// Pause/resume must be accepted while a session is live.
    #[test]
    fn pause_resume_are_accepted() {
        let wav = temp_wav("pause");
        let cfg = LiveConfig {
            mic: MicSelection::default(),
            loopback: LoopbackSelection::default(),
            recorder: RecorderConfig {
                capture_system: false,
                capture_microphone: true,
                ..Default::default()
            },
            wav_path: wav,
        };
        let handle = start_session(cfg).expect("session starts");
        handle.pause();
        handle.resume();
        let _ = handle.stop().expect("stop yields result");
    }
}
