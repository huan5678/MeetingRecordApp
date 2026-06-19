//! PDF exporter. Optional / v1.1 — NOT implemented in v1.0.
//!
//! The v1.1 plan (PRD §4.8) is to render the Markdown export (see
//! [`crate::export::markdown::to_markdown`]) to a styled PDF with an embedded
//! audio-player link. To keep the crate compiling and give callers a clear,
//! non-panicking failure, the entry point exists but returns an error.

use crate::models::{Meeting, Summary, TranscriptSegment};

/// Render a meeting record as a PDF byte stream.
///
/// v1.1 — unimplemented. Returns a clear error so the UI can disable / explain
/// the PDF option in v1.0 instead of panicking.
//
// TODO(v1.1): render `markdown::to_markdown(..)` → styled PDF (CSS theme +
// embedded audio link). Pick a pure-Rust renderer (e.g. `printpdf` or an
// HTML→PDF path) so we don't pull in a headless browser.
pub fn to_pdf(
    _meeting: &Meeting,
    _segments: &[TranscriptSegment],
    _summary: Option<&Summary>,
) -> Result<Vec<u8>, anyhow::Error> {
    Err(anyhow::anyhow!(
        "PDF export is not available in v1.0; planned for v1.1"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Meeting, MeetingStatus};

    fn meeting() -> Meeting {
        Meeting {
            id: "m1".into(),
            title: None,
            start_time: "2026-06-18 14:00".into(),
            end_time: None,
            duration_seconds: None,
            status: MeetingStatus::Completed,
            tags: vec![],
            meeting_type: None,
            created_at: "2026-06-18 14:00:00".into(),
            updated_at: "2026-06-18 14:00:00".into(),
        }
    }

    #[test]
    fn pdf_export_is_unimplemented_in_v1_0() {
        let err = to_pdf(&meeting(), &[], None).unwrap_err();
        assert!(err.to_string().contains("v1.1"));
    }
}
