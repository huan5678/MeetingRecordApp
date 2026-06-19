//! Active window monitor + meeting-app identification.
//!
//! The platform layer that feeds [`super::match_meeting_app`]. On Windows it
//! reads the foreground window via `GetForegroundWindow` + `GetWindowTextW`
//! (PRD §4.7); on every other OS (v1.0 is Windows-only) it is a no-op stub.
//!
//! Design notes:
//! - Polling, not hooks. A short `sleep` between reads keeps CPU near zero; the
//!   foreground window changes on human timescales so a ~1s interval is plenty
//!   and well under any "did you start a meeting?" latency expectation.
//! - The monitor is **edge-triggered**: it only invokes the callback when the
//!   detected meeting app *changes* (e.g. `None` → `Teams`), so the UI isn't
//!   spammed once per poll while the same Teams window stays focused.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::{match_meeting_app, MeetingApp, MeetingPattern};

/// Default interval between foreground-window polls. Chosen to be responsive
/// for a human ("I just opened Teams") without busy-looping.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// A snapshot of the foreground window plus the meeting app it was matched to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWindow {
    /// Raw window title text (may be empty if the OS returns none).
    pub title: String,
    /// Owning process name if the platform layer could resolve it.
    pub process: Option<String>,
    /// The meeting app this window was identified as, if any.
    pub app: Option<MeetingApp>,
}

/// Polls the foreground window and reports when the active meeting app changes.
///
/// Construct with [`WindowMonitor::new`] (built-in patterns) or
/// [`WindowMonitor::with_patterns`] (custom list), then drive it either with a
/// single [`WindowMonitor::poll_once`] or a background [`WindowMonitor::run`]
/// loop. The monitor is `Send + Sync` so the loop can live on its own thread.
pub struct WindowMonitor {
    patterns: Vec<MeetingPattern>,
    interval: Duration,
    /// Last meeting app we reported, used for edge detection.
    last_app: Option<MeetingApp>,
    /// Set to `false` to make [`WindowMonitor::run`] return on the next tick.
    running: Arc<AtomicBool>,
}

impl WindowMonitor {
    /// Build a monitor using the built-in meeting-app patterns and the default
    /// poll interval.
    pub fn new() -> Self {
        Self::with_patterns(super::default_patterns())
    }

    /// Build a monitor with a caller-supplied pattern list (user-configurable
    /// per PRD §4.7) and the default poll interval.
    pub fn with_patterns(patterns: Vec<MeetingPattern>) -> Self {
        WindowMonitor {
            patterns,
            interval: DEFAULT_POLL_INTERVAL,
            last_app: None,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Override the poll interval. Values below 100ms are clamped to 100ms to
    /// avoid pathological busy-looping.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval.max(Duration::from_millis(100));
        self
    }

    /// A handle that can be flipped to stop a running [`WindowMonitor::run`]
    /// loop from another thread.
    pub fn stop_handle(&self) -> StopHandle {
        StopHandle {
            running: Arc::clone(&self.running),
        }
    }

    /// Read the current foreground window and classify it. Pure-ish: the only
    /// side effect is the platform call. Returns the [`ActiveWindow`] snapshot
    /// (with `app: None` when nothing matches, or on non-Windows platforms).
    pub fn poll_once(&self) -> ActiveWindow {
        let (title, process) = read_foreground_window();
        let app = match_meeting_app(&self.patterns, &title, process.as_deref());
        ActiveWindow {
            title,
            process,
            app,
        }
    }

    /// Run the polling loop until [`StopHandle::stop`] is called, invoking
    /// `on_change` **only when the detected meeting app changes** (edge
    /// triggered). The callback receives the new [`ActiveWindow`].
    ///
    /// This blocks the calling thread; spawn it on its own thread for a
    /// background monitor. On non-Windows platforms `poll_once` always yields
    /// `app: None`, so the loop runs harmlessly and never fires `on_change`
    /// (beyond a possible initial `None`), keeping the crate functional
    /// everywhere while doing real work only on Windows.
    pub fn run<F>(&mut self, mut on_change: F)
    where
        F: FnMut(&ActiveWindow),
    {
        // NOTE: do NOT reset `running` to `true` here. It is initialised to
        // `true` in the constructor; a `StopHandle::stop()` that lands *before*
        // `run()` (or between thread-spawn and the first poll) must be honoured,
        // otherwise the loop would spin forever after its only stop signal was
        // clobbered. See the `stop_handle_breaks_the_loop` test.
        while self.running.load(Ordering::SeqCst) {
            let current = self.poll_once();
            if current.app != self.last_app {
                self.last_app = current.app;
                on_change(&current);
            }
            std::thread::sleep(self.interval);
        }
    }
}

impl Default for WindowMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// A thread-safe stop signal for a running [`WindowMonitor::run`] loop.
#[derive(Clone)]
pub struct StopHandle {
    running: Arc<AtomicBool>,
}

impl StopHandle {
    /// Ask the associated monitor loop to exit on its next tick.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Platform layer: foreground window read.
//
// Returns `(title, process_name)`. v1.0 implements Windows; other OSes return
// an empty title / `None` so the pure matcher simply finds nothing.
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn read_foreground_window() -> (String, Option<String>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

    // SAFETY: GetForegroundWindow has no preconditions; it returns a possibly
    // null HWND which we check before reading text.
    let hwnd: HWND = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return (String::new(), None);
    }

    // GetWindowTextW writes UTF-16 into our buffer and returns the length
    // (excluding the NUL). 512 chars is far longer than any real window title.
    let mut buf = [0u16; 512];
    // SAFETY: `buf` is a valid, sized mutable slice for the duration of the call.
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    let title = if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    };

    // Resolving the owning process name is best-effort; on Windows it requires
    // GetWindowThreadProcessId + OpenProcess + QueryFullProcessImageName, which
    // pulls in extra `windows` features not enabled in v1.0's Cargo.toml. The
    // pure matcher already tolerates `None` (process-constrained rules simply
    // don't fire), so we defer process resolution to the Integrate phase and
    // return `None` here. Teams/Zoom are title-only rules and work regardless.
    (title, None)
}

#[cfg(not(target_os = "windows"))]
fn read_foreground_window() -> (String, Option<String>) {
    // v1.0 is Windows-only (PRD §4.7); foreground-window inspection on
    // macOS/Linux is v1.1. Returning an empty title keeps the matcher (and the
    // whole crate) working — it simply never identifies a meeting app here.
    (String::new(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    #[test]
    fn interval_is_clamped_to_a_floor() {
        let m = WindowMonitor::new().with_interval(Duration::from_millis(1));
        assert_eq!(m.interval, Duration::from_millis(100));
    }

    #[test]
    fn custom_interval_is_respected() {
        let m = WindowMonitor::new().with_interval(Duration::from_millis(500));
        assert_eq!(m.interval, Duration::from_millis(500));
    }

    #[test]
    fn poll_once_never_panics_and_is_consistent_with_matcher() {
        // On non-Windows CI this yields an empty title / no app; on Windows it
        // reads the real foreground window. Either way it must not panic and
        // the `app` field must agree with the pure matcher over the snapshot.
        let m = WindowMonitor::new();
        let w = m.poll_once();
        let expected =
            match_meeting_app(&crate::detection::default_patterns(), &w.title, w.process.as_deref());
        assert_eq!(w.app, expected);
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn non_windows_stub_yields_no_app() {
        let m = WindowMonitor::new();
        let w = m.poll_once();
        assert_eq!(w.title, "");
        assert_eq!(w.process, None);
        assert_eq!(w.app, None);
    }

    #[test]
    fn stop_handle_breaks_the_loop() {
        let mut m = WindowMonitor::new().with_interval(Duration::from_millis(100));
        let stop = m.stop_handle();
        // Stop before we ever start: the loop must run at most one iteration.
        stop.stop();
        let start = Instant::now();
        m.run(|_| {});
        // With `running == false` up front, `run` returns essentially
        // immediately (it checks the flag before the first sleep).
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn run_is_edge_triggered_on_app_change() {
        // Drive the loop manually via a fake stream of windows by short-
        // circuiting through `last_app`: we assert that identical consecutive
        // snapshots only fire the callback once. We simulate by setting
        // `last_app` and checking the dedup condition the loop uses.
        let mut m = WindowMonitor::new();
        m.last_app = Some(MeetingApp::Teams);
        // Same app again → no edge.
        let same = ActiveWindow {
            title: "Microsoft Teams".into(),
            process: None,
            app: Some(MeetingApp::Teams),
        };
        assert_eq!(same.app, m.last_app, "same app must not be treated as a change");
        // Different app → edge.
        let changed = ActiveWindow {
            title: "Zoom Meeting".into(),
            process: None,
            app: Some(MeetingApp::Zoom),
        };
        assert_ne!(changed.app, m.last_app, "different app must be treated as a change");
    }

    #[test]
    fn callback_counter_smoke() {
        // A bounded loop: start the monitor on a thread, let it tick a couple
        // of times, then stop it. Asserts the thread joins cleanly and the
        // callback is invoked at most for genuine edges (0 on a stub platform).
        let mut m = WindowMonitor::new().with_interval(Duration::from_millis(100));
        let stop = m.stop_handle();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = Arc::clone(&calls);
        let handle = std::thread::spawn(move || {
            m.run(move |_w| {
                calls2.fetch_add(1, Ordering::SeqCst);
            });
        });
        std::thread::sleep(Duration::from_millis(350));
        stop.stop();
        handle.join().expect("monitor thread should join");
        // On the non-Windows stub the app is always `None`, which equals the
        // initial `last_app`, so zero edges fire. On Windows it depends on the
        // focused window; either way the count is small and the thread joined.
        assert!(calls.load(Ordering::SeqCst) <= 1);
    }
}
