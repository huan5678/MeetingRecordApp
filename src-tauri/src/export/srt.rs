//! SRT / VTT subtitle exporter (pure). See docs/PRD.md §4.8:
//!
//! ```text
//! 1
//! 00:00:00,000 --> 00:00:05,000
//! [Speaker A] Hello everyone
//!
//! 2
//! 00:00:05,000 --> 00:00:10,000
//! [Speaker B] Let's start the meeting
//! ```
//!
//! Both formats are offset-based (timestamps are elapsed time from the start of
//! the recording), so they map directly onto
//! [`TranscriptSegment::start_time_ms`](crate::models::TranscriptSegment) /
//! `end_time_ms`. The only differences between SRT and VTT are the `WEBVTT`
//! header (VTT only) and the `,` vs `.` millisecond separator.

use crate::export::fmt_timestamp;
use crate::models::TranscriptSegment;

/// Render transcript segments as an SRT subtitle file.
///
/// Cues are numbered from 1 in iteration order (the caller is responsible for
/// passing segments already ordered by `segment_index` / `start_time_ms`).
pub fn to_srt(segments: &[TranscriptSegment]) -> String {
    render(segments, SubtitleFormat::Srt)
}

/// Render transcript segments as a WebVTT subtitle file (with the required
/// `WEBVTT` header).
pub fn to_vtt(segments: &[TranscriptSegment]) -> String {
    render(segments, SubtitleFormat::Vtt)
}

#[derive(Clone, Copy)]
enum SubtitleFormat {
    Srt,
    Vtt,
}

impl SubtitleFormat {
    /// Millisecond separator: `,` for SRT, `.` for VTT.
    fn decimal(self) -> char {
        match self {
            SubtitleFormat::Srt => ',',
            SubtitleFormat::Vtt => '.',
        }
    }
}

fn render(segments: &[TranscriptSegment], format: SubtitleFormat) -> String {
    let mut out = String::new();

    if matches!(format, SubtitleFormat::Vtt) {
        // WEBVTT header, separated from the first cue by a blank line.
        out.push_str("WEBVTT\n\n");
    }

    let decimal = format.decimal();
    for (i, seg) in segments.iter().enumerate() {
        // Cue index (1-based).
        out.push_str(&format!("{}\n", i + 1));
        // Time range. Guard against an end before start by clamping end up.
        let end = seg.end_time_ms.max(seg.start_time_ms);
        out.push_str(&format!(
            "{} --> {}\n",
            fmt_timestamp(seg.start_time_ms, decimal),
            fmt_timestamp(end, decimal),
        ));
        // Cue text, prefixed with the speaker label when present.
        match seg.speaker.as_deref().filter(|s| !s.is_empty()) {
            Some(speaker) => out.push_str(&format!("[{speaker}] {}\n", seg.text)),
            None => out.push_str(&format!("{}\n", seg.text)),
        }
        // Blank line terminates every cue (including the last — both players
        // tolerate the trailing newline and most authoring tools emit it).
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TranscriptSegment;

    fn seg(idx: i64, start: i64, end: i64, text: &str, speaker: Option<&str>) -> TranscriptSegment {
        TranscriptSegment {
            id: format!("s{idx}"),
            meeting_id: "m1".into(),
            segment_index: idx,
            start_time_ms: start,
            end_time_ms: end,
            text: text.into(),
            speaker: speaker.map(|s| s.into()),
            confidence: None,
            language: Some("en".into()),
            created_at: "2026-06-18 15:31:00".into(),
        }
    }

    fn fixture() -> Vec<TranscriptSegment> {
        vec![
            seg(0, 0, 5_000, "Hello everyone", Some("Speaker A")),
            seg(1, 5_000, 10_000, "Let's start the meeting", Some("Speaker B")),
        ]
    }

    #[test]
    fn srt_snapshot_matches_prd() {
        let srt = to_srt(&fixture());
        let expected = "1\n\
00:00:00,000 --> 00:00:05,000\n\
[Speaker A] Hello everyone\n\
\n\
2\n\
00:00:05,000 --> 00:00:10,000\n\
[Speaker B] Let's start the meeting\n\
\n";
        assert_eq!(srt, expected);
    }

    #[test]
    fn vtt_snapshot_has_header_and_dot_separator() {
        let vtt = to_vtt(&fixture());
        let expected = "WEBVTT\n\
\n\
1\n\
00:00:00.000 --> 00:00:05.000\n\
[Speaker A] Hello everyone\n\
\n\
2\n\
00:00:05.000 --> 00:00:10.000\n\
[Speaker B] Let's start the meeting\n\
\n";
        assert_eq!(vtt, expected);
    }

    #[test]
    fn segment_without_speaker_has_no_bracket_prefix() {
        let segs = vec![seg(0, 0, 2_000, "Anonymous line", None)];
        let srt = to_srt(&segs);
        assert!(srt.contains("\nAnonymous line\n"));
        assert!(!srt.contains("[]"));
    }

    #[test]
    fn empty_input_produces_empty_srt_and_header_only_vtt() {
        assert_eq!(to_srt(&[]), "");
        assert_eq!(to_vtt(&[]), "WEBVTT\n\n");
    }

    #[test]
    fn end_before_start_is_clamped() {
        let segs = vec![seg(0, 5_000, 1_000, "oops", None)];
        let srt = to_srt(&segs);
        // End clamps up to the start instead of going backwards.
        assert!(srt.contains("00:00:05,000 --> 00:00:05,000"));
    }
}
