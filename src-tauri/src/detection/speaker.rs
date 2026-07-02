//! Speaker identity stream (spec 2026-07-03): capture *who* is speaking and
//! *when*, independently of transcription, then reconcile with the transcript
//! by a mechanical overlap-join.
//!
//! First principles: the loopback mix is identity-blind, so identity must come
//! from a separate, timestamped signal captured *during* recording. This module
//! holds the **pure, testable** core of that pipeline:
//!
//! - [`build_spans`] — active-speaker samples → merged `(name, start, end)` spans.
//! - [`to_speaker_srt`] / [`parse_speaker_srt`] — the on-disk sidecar (`speakers.srt`).
//! - [`assign_speakers`] — overlap-join spans onto transcript segments.
//!
//! The Windows UI Automation poller that *produces* the samples is the only
//! platform-specific, un-unit-testable piece; it is gated on the Phase 0 spike
//! (see the spec) and lives at the bottom behind `#[cfg(target_os = "windows")]`.

use crate::models::TranscriptSegment;

/// A contiguous stretch of time attributed to one speaker (display name).
/// Timestamps are milliseconds elapsed from the start of the recording — the
/// **same clock** as [`TranscriptSegment::start_time_ms`], so the two align.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerSpan {
    pub name: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// Debounce tuning for [`build_spans`]. Defaults are the spec's real-world
/// values; they are the knobs to turn when Teams' active-speaker latency/flicker
/// looks different in the field.
#[derive(Debug, Clone, Copy)]
pub struct SpanConfig {
    /// Poll interval the samples were taken at. Each sample represents this much
    /// speaking time, so a span's `end` is its last sample time plus `poll_ms`.
    pub poll_ms: i64,
    /// Spans shorter than this are dropped as active-speaker flicker.
    pub min_span_ms: i64,
    /// A gap between consecutive same-name samples larger than this closes the
    /// span (a silence happened in between).
    pub max_gap_ms: i64,
}

impl Default for SpanConfig {
    fn default() -> Self {
        // ponytail: field-tunable. active-speaker has ~0.5-1s latency and only
        // reports the *dominant* speaker; these values trade flicker vs latency.
        SpanConfig {
            poll_ms: 250,
            min_span_ms: 600,
            max_gap_ms: 750,
        }
    }
}

/// Collapse a time-ordered stream of active-speaker samples into speaker spans.
///
/// `samples` is `(elapsed_ms, Some(name) | None)`; `None` means "nobody / could
/// not read". Consecutive same-name samples merge; a different name, a `None`,
/// or a gap larger than `max_gap_ms` closes the current span; spans shorter than
/// `min_span_ms` are dropped as flicker.
pub fn build_spans(samples: &[(i64, Option<String>)], cfg: &SpanConfig) -> Vec<SpeakerSpan> {
    let mut spans = Vec::new();
    // The span currently being accumulated: (name, start_ms, last_sample_ms).
    let mut open: Option<(String, i64, i64)> = None;

    // Close `open` into `spans` if it is long enough, using `poll_ms` to turn
    // the last sample time into an end boundary.
    let flush = |open: &mut Option<(String, i64, i64)>, spans: &mut Vec<SpeakerSpan>| {
        if let Some((name, start, last)) = open.take() {
            let end = last + cfg.poll_ms;
            if end - start >= cfg.min_span_ms {
                spans.push(SpeakerSpan { name, start_ms: start, end_ms: end });
            }
        }
    };

    for (t, sample) in samples {
        match sample {
            None => flush(&mut open, &mut spans),
            Some(name) => match &open {
                // Same speaker, no gap: extend.
                Some((cur, _, last)) if cur == name && t - last <= cfg.max_gap_ms => {
                    if let Some(o) = open.as_mut() {
                        o.2 = *t;
                    }
                }
                // Different speaker, or a gap too large: close and start fresh.
                _ => {
                    flush(&mut open, &mut spans);
                    open = Some((name.clone(), *t, *t));
                }
            },
        }
    }
    flush(&mut open, &mut spans);
    spans
}

/// Render speaker spans as an SRT sidecar (`speakers.srt`): each cue's text is
/// the speaker name. Reuses the export timestamp formatter so it is byte-for-byte
/// the same subtitle grammar as the transcript SRT.
pub fn to_speaker_srt(spans: &[SpeakerSpan]) -> String {
    use crate::export::fmt_timestamp;
    let mut out = String::new();
    for (i, sp) in spans.iter().enumerate() {
        let end = sp.end_ms.max(sp.start_ms);
        out.push_str(&format!("{}\n", i + 1));
        out.push_str(&format!(
            "{} --> {}\n",
            fmt_timestamp(sp.start_ms, ','),
            fmt_timestamp(end, ','),
        ));
        out.push_str(&format!("{}\n\n", sp.name));
    }
    out
}

/// Parse a `speakers.srt` sidecar back into spans. Tolerant of blank lines and
/// the trailing newline; ignores any cue it cannot parse rather than failing the
/// whole file.
pub fn parse_speaker_srt(text: &str) -> Vec<SpeakerSpan> {
    let mut spans = Vec::new();
    // Cues are separated by blank lines. Within a cue we look for the one line
    // containing "-->" (the time range); the last non-empty line is the name.
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let Some(time_idx) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let Some((start, end)) = parse_time_range(lines[time_idx]) else {
            continue;
        };
        // Name = everything after the time line, joined (names shouldn't wrap,
        // but be tolerant); skip cues with no name.
        let name = lines[time_idx + 1..].join(" ");
        if name.is_empty() {
            continue;
        }
        spans.push(SpeakerSpan { name, start_ms: start, end_ms: end });
    }
    spans
}

/// Default share of a segment that must be covered by one speaker span before we
/// attribute the segment to that speaker (spec knob).
pub const DEFAULT_MIN_OVERLAP_FRAC: f64 = 0.5;

/// Fill each segment's `speaker` from the identity spans by temporal overlap.
///
/// For every segment, the span with the largest overlap wins (ties break toward
/// the earlier span). The name is written only when that overlap covers at least
/// `min_overlap_frac` of the segment's duration; otherwise the segment's existing
/// label (diarization's `Speaker N`, or `None`) is left untouched. Deterministic
/// — no LLM guessing.
pub fn assign_speakers(
    segments: &mut [TranscriptSegment],
    spans: &[SpeakerSpan],
    min_overlap_frac: f64,
) {
    for seg in segments.iter_mut() {
        let dur = seg.end_time_ms - seg.start_time_ms;
        if dur <= 0 {
            continue; // zero/negative-length segment: nothing to attribute
        }
        // Best overlap so far; strictly-greater replaces, so ties keep the
        // earlier span (spans are passed in start order).
        let mut best: Option<(&SpeakerSpan, i64)> = None;
        for sp in spans {
            let overlap = (seg.end_time_ms.min(sp.end_ms) - seg.start_time_ms.max(sp.start_ms)).max(0);
            if overlap <= 0 {
                continue;
            }
            if best.map_or(true, |(_, b)| overlap > b) {
                best = Some((sp, overlap));
            }
        }
        if let Some((sp, overlap)) = best {
            if overlap as f64 >= min_overlap_frac * dur as f64 {
                seg.speaker = Some(sp.name.clone());
            }
        }
    }
}

/// Parse `HH:MM:SS,mmm --> HH:MM:SS,mmm` into `(start_ms, end_ms)`.
fn parse_time_range(line: &str) -> Option<(i64, i64)> {
    let (a, b) = line.split_once("-->")?;
    Some((parse_ts(a.trim())?, parse_ts(b.trim())?))
}

/// Parse a single `HH:MM:SS,mmm` (or `.mmm`) timestamp into milliseconds.
fn parse_ts(s: &str) -> Option<i64> {
    let (hms, millis) = s.split_once([',', '.'])?;
    let mut parts = hms.split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let sec: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let ms: i64 = millis.parse().ok()?;
    Some(((h * 3_600 + m * 60 + sec) * 1_000) + ms)
}

// ---------------------------------------------------------------------------
// U1 — the active-speaker poller (platform layer).
//
// Mirrors [`crate::detection::monitor::WindowMonitor`]: poll on an interval,
// stop via an atomic flag set from another thread, collect the sample stream,
// then debounce into spans. The "who is speaking in Teams" read itself is
// Windows-only and gated on the Phase 0 UIA spike (see the spec).
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default active-speaker poll interval (4 Hz); matches [`SpanConfig::poll_ms`].
pub const DEFAULT_POLL_MS: i64 = 250;

/// Polls the current Teams active speaker on a fixed interval, collecting a
/// sample stream that [`SpeakerMonitor::run_and_collect`] debounces into spans.
pub struct SpeakerMonitor {
    /// Recording's monotonic clock base — sample times are `now - start`, the
    /// same base as the audio/transcript offsets, so the two align.
    start: Instant,
    interval: Duration,
    cfg: SpanConfig,
    running: Arc<AtomicBool>,
    samples: Vec<(i64, Option<String>)>,
}

impl SpeakerMonitor {
    /// Pass the instant the audio capture started so identity timestamps share
    /// the recording clock.
    pub fn new(start: Instant) -> Self {
        SpeakerMonitor {
            start,
            interval: Duration::from_millis(DEFAULT_POLL_MS as u64),
            cfg: SpanConfig::default(),
            running: Arc::new(AtomicBool::new(true)),
            samples: Vec::new(),
        }
    }

    /// Override the poll interval (also the span builder's `poll_ms`). Floored at
    /// 50 ms to avoid busy-looping.
    pub fn with_interval_ms(mut self, ms: i64) -> Self {
        let ms = ms.max(50);
        self.interval = Duration::from_millis(ms as u64);
        self.cfg.poll_ms = ms;
        self
    }

    /// A thread-safe stop signal for a running [`SpeakerMonitor::run_and_collect`].
    pub fn stop_handle(&self) -> SpeakerStopHandle {
        SpeakerStopHandle {
            running: Arc::clone(&self.running),
        }
    }

    /// Poll until stopped, then return the debounced spans. Blocks — spawn it on
    /// its own thread, drive the recording, call [`SpeakerStopHandle::stop`], then
    /// join to get the spans to persist as `speakers.srt`.
    pub fn run_and_collect(mut self) -> Vec<SpeakerSpan> {
        while self.running.load(Ordering::SeqCst) {
            let elapsed = self.start.elapsed().as_millis() as i64;
            self.samples.push((elapsed, read_active_speaker()));
            std::thread::sleep(self.interval);
        }
        build_spans(&self.samples, &self.cfg)
    }
}

/// Stop signal for a running [`SpeakerMonitor`].
#[derive(Clone)]
pub struct SpeakerStopHandle {
    running: Arc<AtomicBool>,
}

impl SpeakerStopHandle {
    /// Ask the monitor loop to exit on its next tick.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// A running speaker poller owned by the recording session: spawn on
/// record-start, [`finish`](SpeakerCapture::finish) on stop to get the spans to
/// persist as `speakers.srt`.
pub struct SpeakerCapture {
    stop: SpeakerStopHandle,
    handle: std::thread::JoinHandle<Vec<SpeakerSpan>>,
}

impl SpeakerCapture {
    /// Spawn the poller on its own thread, clocked from `start` (pass the
    /// recording's start instant so identity times align with audio offsets).
    pub fn spawn(start: Instant) -> Self {
        let monitor = SpeakerMonitor::new(start);
        let stop = monitor.stop_handle();
        let handle = std::thread::spawn(move || monitor.run_and_collect());
        SpeakerCapture { stop, handle }
    }

    /// Stop polling and return the debounced spans (empty if nothing was
    /// captured — e.g. non-Teams, non-Windows, or before the Phase 0 UIA read
    /// lands). Never panics: a poisoned poller thread yields no spans.
    pub fn finish(self) -> Vec<SpeakerSpan> {
        self.stop.stop();
        self.handle.join().unwrap_or_default()
    }
}

/// The current Teams active-speaker display name, or `None`.
#[cfg(target_os = "windows")]
fn read_active_speaker() -> Option<String> {
    // PHASE 0 GATE (spec 2026-07-03): reading the "current speaker" name from new
    // Teams (WebView2) via UI Automation is UNVERIFIED. Shipping a guessed tree
    // walk risks a broken Windows build, so this returns `None` until the spike
    // confirms the exact UIA element + property. Verified plan once it passes:
    //   1. Confirm the foreground window is Microsoft Teams (reuse detection).
    //   2. Walk the Teams UIA subtree to the main-stage active-speaker name label.
    //   3. Return its UIA `Name` (display name), or `None` when off-stage/unread.
    None
}

#[cfg(not(target_os = "windows"))]
fn read_active_speaker() -> Option<String> {
    // v1.0 is Windows-only; elsewhere there is no Teams desktop client to read.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn s(t: i64, name: Option<&str>) -> (i64, Option<String>) {
        (t, name.map(|n| n.to_string()))
    }

    #[test]
    fn build_spans_merges_drops_flicker_and_closes_on_none() {
        let cfg = SpanConfig {
            poll_ms: 100,
            min_span_ms: 150,
            max_gap_ms: 150,
        };
        let samples = vec![
            s(0, Some("Alice")),
            s(100, Some("Alice")), // Alice run 0..100 -> span [0,200) dur 200 keep
            s(200, Some("Bob")),   // single Bob -> span [200,300) dur 100 < 150 drop
            s(300, Some("Carol")),
            s(400, Some("Carol")), // Carol run 300..400 -> span [300,500) dur 200 keep
            s(500, None),          // closes Carol
            s(600, Some("Dave")),  // single Dave -> [600,700) dur 100 drop
        ];
        let spans = build_spans(&samples, &cfg);
        assert_eq!(
            spans,
            vec![
                SpeakerSpan { name: "Alice".into(), start_ms: 0, end_ms: 200 },
                SpeakerSpan { name: "Carol".into(), start_ms: 300, end_ms: 500 },
            ]
        );
    }

    fn span(name: &str, start: i64, end: i64) -> SpeakerSpan {
        SpeakerSpan { name: name.into(), start_ms: start, end_ms: end }
    }

    #[test]
    fn to_speaker_srt_matches_subtitle_grammar() {
        let spans = vec![span("Alice Chen", 3_200, 11_750), span("Bob Wang", 11_750, 15_000)];
        let expected = "1\n\
00:00:03,200 --> 00:00:11,750\n\
Alice Chen\n\
\n\
2\n\
00:00:11,750 --> 00:00:15,000\n\
Bob Wang\n\
\n";
        assert_eq!(to_speaker_srt(&spans), expected);
    }

    #[test]
    fn speaker_srt_round_trips() {
        let spans = vec![
            span("Alice Chen", 0, 4_000),
            span("Bob Wang", 4_000, 12_500),
            span("周小明", 12_500, 20_007), // non-ASCII name survives
        ];
        assert_eq!(parse_speaker_srt(&to_speaker_srt(&spans)), spans);
    }

    #[test]
    fn parse_speaker_srt_ignores_garbage_cues() {
        // A malformed timestamp line must not kill the whole file.
        let text = "1\nnot a timestamp\nAlice\n\n2\n00:00:05,000 --> 00:00:06,000\nBob\n\n";
        assert_eq!(parse_speaker_srt(text), vec![span("Bob", 5_000, 6_000)]);
    }

    fn seg(start: i64, end: i64, speaker: Option<&str>) -> TranscriptSegment {
        TranscriptSegment {
            id: format!("s{start}"),
            meeting_id: "m1".into(),
            segment_index: start,
            start_time_ms: start,
            end_time_ms: end,
            text: "…".into(),
            speaker: speaker.map(|s| s.into()),
            confidence: None,
            language: None,
            created_at: "2026-07-03 00:00:00".into(),
        }
    }

    #[test]
    fn assign_speakers_overlap_join() {
        let spans = vec![span("Alice", 0, 4_000), span("Bob", 4_000, 12_000)];
        let mut segs = vec![
            seg(0, 5_000, None),                  // Alice 4000 vs Bob 1000 -> Alice
            seg(3_000, 7_000, None),              // Alice 1000 vs Bob 3000 -> Bob (straddle majority)
            seg(3_500, 4_500, None),              // 500 vs 500 tie -> earlier span (Alice)
            seg(11_000, 20_000, Some("Speaker 2")), // Bob 1000 / 9000 = 0.11 < 0.5 -> keep Speaker 2
            seg(13_000, 14_000, None),            // no overlap -> stays None
        ];
        assign_speakers(&mut segs, &spans, DEFAULT_MIN_OVERLAP_FRAC);
        let got: Vec<Option<&str>> = segs.iter().map(|s| s.speaker.as_deref()).collect();
        assert_eq!(
            got,
            vec![Some("Alice"), Some("Bob"), Some("Alice"), Some("Speaker 2"), None]
        );
    }

    #[test]
    fn speaker_monitor_stops_and_yields_no_spans_when_stopped_immediately() {
        // Lifecycle check (the only Mac-runnable part of U1): stopping before the
        // loop runs yields no samples, hence no spans, and never hangs. On the
        // non-Windows stub read_active_speaker() is always None regardless.
        let mon = SpeakerMonitor::new(Instant::now()).with_interval_ms(50);
        let stop = mon.stop_handle();
        stop.stop();
        assert!(mon.run_and_collect().is_empty());
    }

    #[test]
    fn speaker_capture_spawn_finish_joins_cleanly() {
        // The session-facing wrapper must spawn and join without hanging. Off
        // Windows (and pre-Phase-0) the read yields None, so spans are empty.
        let cap = SpeakerCapture::spawn(Instant::now());
        assert!(cap.finish().is_empty());
    }

    #[test]
    fn build_spans_splits_on_gap() {
        let cfg = SpanConfig {
            poll_ms: 100,
            min_span_ms: 150,
            max_gap_ms: 150,
        };
        let samples = vec![
            s(0, Some("Alice")),
            s(100, Some("Alice")), // [0,200)
            s(400, Some("Alice")), // gap 300 > 150 -> new Alice run starts at 400
            s(500, Some("Alice")), // [400,600)
        ];
        let spans = build_spans(&samples, &cfg);
        assert_eq!(
            spans,
            vec![
                SpeakerSpan { name: "Alice".into(), start_ms: 0, end_ms: 200 },
                SpeakerSpan { name: "Alice".into(), start_ms: 400, end_ms: 600 },
            ]
        );
    }
}
