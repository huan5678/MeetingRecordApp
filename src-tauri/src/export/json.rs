//! JSON exporter (pure, full structured data). See docs/PRD.md §4.8:
//! "Full structured data export (meeting, transcript, summary, metadata)".
//!
//! The output is a single object combining the meeting, its transcript
//! segments, and (optionally) the summary, plus an `export_version` marker so
//! downstream/integration tooling can detect the schema it's reading. Because
//! every domain type in [`crate::models`] already derives `Serialize`, this is
//! a thin, deterministic wrapper around `serde_json`.

use serde::Serialize;

use crate::models::{Meeting, Summary, TranscriptSegment};

/// Schema version for the JSON export envelope. Bump on breaking changes.
pub const EXPORT_VERSION: u32 = 1;

/// The top-level export envelope. Borrows its inputs so callers don't have to
/// clone large transcripts just to serialize them.
#[derive(Debug, Serialize)]
pub struct MeetingExport<'a> {
    /// Schema marker for consumers (see [`EXPORT_VERSION`]).
    pub export_version: u32,
    pub meeting: &'a Meeting,
    pub segments: &'a [TranscriptSegment],
    /// Present only when a summary was generated for the meeting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<&'a Summary>,
}

/// Build the export envelope without serializing it (useful if a caller wants
/// to embed it in a larger structure).
pub fn build_export<'a>(
    meeting: &'a Meeting,
    segments: &'a [TranscriptSegment],
    summary: Option<&'a Summary>,
) -> MeetingExport<'a> {
    MeetingExport {
        export_version: EXPORT_VERSION,
        meeting,
        segments,
        summary,
    }
}

/// Render a meeting record as pretty-printed JSON.
///
/// Returns `Err` only if serialization fails, which for these `#[derive]`d
/// `Serialize` types is effectively impossible (no custom `serialize` impls
/// that can error); the `Result` is kept so the signature is honest.
pub fn to_json(
    meeting: &Meeting,
    segments: &[TranscriptSegment],
    summary: Option<&Summary>,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&build_export(meeting, segments, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ActionItem, KeyDecision, Meeting, MeetingStatus, MeetingType, Summary, SummaryType,
        TranscriptSegment,
    };

    fn meeting() -> Meeting {
        Meeting {
            id: "m1".into(),
            title: Some("Weekly Sync".into()),
            start_time: "2026-06-18 14:00".into(),
            end_time: Some("2026-06-18 15:30".into()),
            duration_seconds: Some(5_400),
            status: MeetingStatus::Completed,
            tags: vec!["eng".into()],
            meeting_type: Some(MeetingType::TeamSync),
            created_at: "2026-06-18 14:00:00".into(),
            updated_at: "2026-06-18 15:31:00".into(),
        }
    }

    fn segments() -> Vec<TranscriptSegment> {
        vec![TranscriptSegment {
            id: "s1".into(),
            meeting_id: "m1".into(),
            segment_index: 0,
            start_time_ms: 0,
            end_time_ms: 5_000,
            text: "Hello everyone".into(),
            speaker: Some("Speaker A".into()),
            confidence: Some(0.98),
            language: Some("en".into()),
            created_at: "2026-06-18 15:31:00".into(),
        }]
    }

    fn summary() -> Summary {
        Summary {
            id: "sum1".into(),
            meeting_id: "m1".into(),
            summary_type: SummaryType::Auto,
            content: "Discussed the release.".into(),
            action_items: vec![ActionItem {
                task: "Ship it".into(),
                owner: Some("Alice".into()),
                deadline: Some("2026-06-20".into()),
                done: false,
            }],
            key_decisions: vec![KeyDecision {
                decision: "Windows-only".into(),
                context: None,
            }],
            prompt_used: None,
            ai_provider: None,
            ai_model: None,
            tokens_used: None,
            created_at: "2026-06-18 15:31:00".into(),
        }
    }

    #[test]
    fn json_round_trips_back_into_the_domain_types() {
        // Snapshot-by-structure: serialize, then re-parse the parts and assert
        // equality on the meaningful fields. This is more robust than pinning
        // exact whitespace while still proving the shape.
        let json = to_json(&meeting(), &segments(), Some(&summary())).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["export_version"], 1);
        assert_eq!(value["meeting"]["id"], "m1");
        assert_eq!(value["meeting"]["status"], "completed");
        assert_eq!(value["meeting"]["meeting_type"], "team_sync");
        assert_eq!(value["segments"][0]["text"], "Hello everyone");
        assert_eq!(value["segments"][0]["speaker"], "Speaker A");
        assert_eq!(value["summary"]["summary_type"], "auto");
        assert_eq!(value["summary"]["action_items"][0]["owner"], "Alice");
        assert_eq!(value["summary"]["key_decisions"][0]["decision"], "Windows-only");
    }

    #[test]
    fn summary_omitted_when_none() {
        let json = to_json(&meeting(), &segments(), None).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("summary").is_none());
    }

    #[test]
    fn one_on_one_serializes_as_1on1() {
        let mut m = meeting();
        m.meeting_type = Some(MeetingType::OneOnOne);
        let json = to_json(&m, &[], None).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["meeting"]["meeting_type"], "1on1");
    }

    #[test]
    fn output_is_pretty_printed() {
        let json = to_json(&meeting(), &segments(), None).unwrap();
        // Pretty printing means newlines + indentation are present.
        assert!(json.contains("\n  \"export_version\": 1"));
    }
}
