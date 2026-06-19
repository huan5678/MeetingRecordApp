//! Export module.
//!
//! Pure functions: meeting data → Markdown / SRT/VTT / JSON (PDF + Notion are
//! optional / v1.1). Snapshot-friendly tests. See docs/PRD.md §4.8.
//!
//! Every public exporter is a *pure* function over the shared domain types in
//! [`crate::models`] — no I/O, no clock, no global state — so its output is
//! deterministic and trivially snapshot-tested. Writing the resulting `String`
//! to disk is the caller's job (the storage / command layer).
//!
//! ## A note on transcript timestamps
//! The PRD §4.8 Markdown sample shows wall-clock times (`[14:00:00]`).
//! [`TranscriptSegment`](crate::models::TranscriptSegment) only carries
//! millisecond offsets *relative to the start of the recording*, so to stay a
//! pure function (no date math / no `chrono`) the exporters render the **elapsed
//! offset** as `[HH:MM:SS]`. The first segment of a meeting therefore starts at
//! `[00:00:00]`. The subtitle formats (SRT/VTT) are offset-based by definition,
//! so they match the spec exactly.

pub mod json;
pub mod markdown;
pub mod notion;
pub mod pdf;
pub mod srt;

// Re-export the entry points so callers can write `export::to_markdown(..)`.
pub use json::to_json;
pub use markdown::to_markdown;
pub use srt::{to_srt, to_vtt};

/// The set of formats the UI offers (PRD §3.6 / §4.8). `Pdf` and `Notion` are
/// declared for the v1.1+ surface but their exporters are stubs in v1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Srt,
    Vtt,
    Json,
    /// v1.1 — see [`pdf`].
    Pdf,
    /// v1.2 — see [`notion`].
    Notion,
}

impl ExportFormat {
    /// Conventional file extension (without the dot) for this format.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Srt => "srt",
            ExportFormat::Vtt => "vtt",
            ExportFormat::Json => "json",
            ExportFormat::Pdf => "pdf",
            ExportFormat::Notion => "md",
        }
    }
}

// ---------------------------------------------------------------------------
// Shared formatting helpers used by more than one exporter.
// ---------------------------------------------------------------------------

/// Format a millisecond offset as `HH:MM:SS` (hours never zero-truncated, so a
/// 90-minute meeting reads `01:30:00`). Negative input is clamped to zero.
pub(crate) fn fmt_clock(ms: i64) -> String {
    let total_secs = ms.max(0) / 1_000;
    let h = total_secs / 3_600;
    let m = (total_secs % 3_600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

/// Format a millisecond offset as `HH:MM:SS,mmm` (SRT) or `HH:MM:SS.mmm` (VTT),
/// chosen by `decimal` (`,` for SRT, `.` for VTT). Negative input clamps to zero.
pub(crate) fn fmt_timestamp(ms: i64, decimal: char) -> String {
    let ms = ms.max(0);
    let millis = ms % 1_000;
    let total_secs = ms / 1_000;
    let h = total_secs / 3_600;
    let m = (total_secs % 3_600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}{decimal}{millis:03}")
}

/// Human-friendly duration like `1h 30m`, `45m`, `5m 3s`, `12s`. `None` yields
/// `"—"`. Used in the Markdown header (PRD §4.8 shows `1h 30m`).
pub(crate) fn fmt_duration(seconds: Option<i64>) -> String {
    let Some(total) = seconds else {
        return "—".to_string();
    };
    if total <= 0 {
        return "0s".to_string();
    }
    let h = total / 3_600;
    let m = (total % 3_600) / 60;
    let s = total % 60;
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h}h"));
    }
    if m > 0 {
        parts.push(format!("{m}m"));
    }
    // Only surface seconds when there's no hour component (matches the PRD's
    // coarse `1h 30m`; a sub-minute meeting still shows something useful).
    if s > 0 && h == 0 {
        parts.push(format!("{s}s"));
    }
    if parts.is_empty() {
        // e.g. exactly 1h with 0m/0s, or a value that rounded everything away.
        return format!("{h}h");
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_clock_renders_hms() {
        assert_eq!(fmt_clock(0), "00:00:00");
        assert_eq!(fmt_clock(5_000), "00:00:05");
        assert_eq!(fmt_clock(65_000), "00:01:05");
        assert_eq!(fmt_clock(3_661_000), "01:01:01");
        assert_eq!(fmt_clock(-10), "00:00:00");
    }

    #[test]
    fn fmt_timestamp_srt_and_vtt_separators() {
        assert_eq!(fmt_timestamp(0, ','), "00:00:00,000");
        assert_eq!(fmt_timestamp(0, '.'), "00:00:00.000");
        assert_eq!(fmt_timestamp(5_250, ','), "00:00:05,250");
        assert_eq!(fmt_timestamp(3_661_007, '.'), "01:01:01.007");
        assert_eq!(fmt_timestamp(-1, ','), "00:00:00,000");
    }

    #[test]
    fn fmt_duration_variants() {
        assert_eq!(fmt_duration(Some(5_400)), "1h 30m");
        assert_eq!(fmt_duration(Some(2_700)), "45m");
        assert_eq!(fmt_duration(Some(303)), "5m 3s");
        assert_eq!(fmt_duration(Some(12)), "12s");
        assert_eq!(fmt_duration(Some(3_600)), "1h");
        assert_eq!(fmt_duration(Some(0)), "0s");
        assert_eq!(fmt_duration(None), "—");
    }

    #[test]
    fn export_format_extensions() {
        assert_eq!(ExportFormat::Markdown.extension(), "md");
        assert_eq!(ExportFormat::Srt.extension(), "srt");
        assert_eq!(ExportFormat::Vtt.extension(), "vtt");
        assert_eq!(ExportFormat::Json.extension(), "json");
        assert_eq!(ExportFormat::Pdf.extension(), "pdf");
    }
}
