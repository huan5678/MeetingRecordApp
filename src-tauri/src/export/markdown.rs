//! Markdown exporter (pure). Renders a meeting to the Markdown layout in
//! docs/PRD.md §4.8:
//!
//! ```markdown
//! # Meeting: [Title]
//! **Date:** 2026-06-18 14:00 - 15:30
//! **Duration:** 1h 30m
//! **Type:** Team Sync
//!
//! ## Summary
//! [AI-generated summary]
//!
//! ## Action Items
//! - [ ] [Owner] [Task] - Due: [Date]
//!
//! ## Key Decisions
//! 1. [Decision 1]
//!
//! ## Transcript
//! [14:00:00] Speaker A: [text]
//! ```
//!
//! Pure: `(&Meeting, &[TranscriptSegment], Option<&Summary>) -> String`.

use crate::export::{fmt_clock, fmt_duration};
use crate::models::{Meeting, Summary, TranscriptSegment};

/// Two trailing spaces force a hard line break in CommonMark, matching the
/// PRD's header block where each metadata line is its own visual line.
const HARD_BREAK: &str = "  ";

/// Render a complete meeting record as Markdown.
///
/// `summary` is optional: a meeting may be exported before AI summarization has
/// run (or with it disabled), in which case the Summary / Action Items / Key
/// Decisions sections are omitted entirely.
pub fn to_markdown(
    meeting: &Meeting,
    segments: &[TranscriptSegment],
    summary: Option<&Summary>,
) -> String {
    let mut out = String::new();

    // --- Header --------------------------------------------------------------
    let title = meeting.title.as_deref().unwrap_or("Untitled Meeting");
    out.push_str(&format!("# Meeting: {title}\n"));

    out.push_str(&format!(
        "**Date:** {}{}\n",
        fmt_date_range(meeting),
        HARD_BREAK
    ));
    out.push_str(&format!(
        "**Duration:** {}{}\n",
        fmt_duration(meeting.duration_seconds),
        HARD_BREAK
    ));
    if let Some(t) = meeting.meeting_type {
        out.push_str(&format!("**Type:** {}\n", meeting_type_label(t)));
    }
    if !meeting.tags.is_empty() {
        out.push_str(&format!("**Tags:** {}\n", meeting.tags.join(", ")));
    }

    // --- Summary block (only when a summary exists) --------------------------
    if let Some(summary) = summary {
        if !summary.content.trim().is_empty() {
            out.push_str("\n## Summary\n");
            out.push_str(summary.content.trim_end());
            out.push('\n');
        }

        if !summary.action_items.is_empty() {
            out.push_str("\n## Action Items\n");
            for item in &summary.action_items {
                let checkbox = if item.done { "[x]" } else { "[ ]" };
                let owner = item
                    .owner
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|o| format!("**{o}** "))
                    .unwrap_or_default();
                let due = item
                    .deadline
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|d| format!(" - Due: {d}"))
                    .unwrap_or_default();
                out.push_str(&format!("- {checkbox} {owner}{}{due}\n", item.task));
            }
        }

        if !summary.key_decisions.is_empty() {
            out.push_str("\n## Key Decisions\n");
            for (i, decision) in summary.key_decisions.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", i + 1, decision.decision));
            }
        }
    }

    // --- Transcript ----------------------------------------------------------
    out.push_str("\n## Transcript\n");
    if segments.is_empty() {
        out.push_str("_No transcript available._\n");
    } else {
        for seg in segments {
            let ts = fmt_clock(seg.start_time_ms);
            match seg.speaker.as_deref().filter(|s| !s.is_empty()) {
                Some(speaker) => {
                    out.push_str(&format!("[{ts}] {speaker}: {}\n", seg.text));
                }
                None => {
                    out.push_str(&format!("[{ts}] {}\n", seg.text));
                }
            }
        }
    }

    out
}

/// `start - end` using the raw stored timestamps. We deliberately don't parse
/// them (no `chrono` dependency); the storage layer writes ISO-8601, which is
/// already human-readable. End time falls back to `…` when the meeting has no
/// recorded end (e.g. still recording).
fn fmt_date_range(meeting: &Meeting) -> String {
    match meeting.end_time.as_deref() {
        Some(end) if !end.is_empty() => format!("{} - {}", meeting.start_time, end),
        _ => meeting.start_time.clone(),
    }
}

/// Human label for the meeting type used in the header (PRD §4.8 shows
/// `Team Sync`, title-cased rather than the DB snake_case).
fn meeting_type_label(t: crate::models::MeetingType) -> &'static str {
    use crate::models::MeetingType::*;
    match t {
        OneOnOne => "1:1",
        TeamSync => "Team Sync",
        ClientCall => "Client Call",
        Interview => "Interview",
        Other => "Other",
    }
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
            tags: vec!["project-x".into(), "eng".into()],
            meeting_type: Some(MeetingType::TeamSync),
            created_at: "2026-06-18 14:00:00".into(),
            updated_at: "2026-06-18 15:31:00".into(),
        }
    }

    fn segments() -> Vec<TranscriptSegment> {
        vec![
            TranscriptSegment {
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
            },
            TranscriptSegment {
                id: "s2".into(),
                meeting_id: "m1".into(),
                segment_index: 1,
                start_time_ms: 5_000,
                end_time_ms: 10_000,
                text: "Let's start the meeting".into(),
                speaker: Some("Speaker B".into()),
                confidence: Some(0.95),
                language: Some("en".into()),
                created_at: "2026-06-18 15:31:00".into(),
            },
        ]
    }

    fn summary() -> Summary {
        Summary {
            id: "sum1".into(),
            meeting_id: "m1".into(),
            summary_type: SummaryType::Auto,
            content: "The team discussed the v1.0 release plan.".into(),
            action_items: vec![
                ActionItem {
                    task: "Finish export module".into(),
                    owner: Some("Alice".into()),
                    deadline: Some("2026-06-20".into()),
                    done: false,
                },
                ActionItem {
                    task: "Review PR".into(),
                    owner: None,
                    deadline: None,
                    done: true,
                },
            ],
            key_decisions: vec![
                KeyDecision {
                    decision: "Ship Windows-only for v1.0".into(),
                    context: None,
                },
                KeyDecision {
                    decision: "Default to Ollama for summaries".into(),
                    context: Some("privacy".into()),
                },
            ],
            prompt_used: None,
            ai_provider: None,
            ai_model: None,
            tokens_used: None,
            created_at: "2026-06-18 15:31:00".into(),
        }
    }

    #[test]
    fn full_markdown_snapshot() {
        let md = to_markdown(&meeting(), &segments(), Some(&summary()));
        let expected = "# Meeting: Weekly Sync\n\
**Date:** 2026-06-18 14:00 - 2026-06-18 15:30  \n\
**Duration:** 1h 30m  \n\
**Type:** Team Sync\n\
**Tags:** project-x, eng\n\
\n## Summary\n\
The team discussed the v1.0 release plan.\n\
\n## Action Items\n\
- [ ] **Alice** Finish export module - Due: 2026-06-20\n\
- [x] Review PR\n\
\n## Key Decisions\n\
1. Ship Windows-only for v1.0\n\
2. Default to Ollama for summaries\n\
\n## Transcript\n\
[00:00:00] Speaker A: Hello everyone\n\
[00:00:05] Speaker B: Let's start the meeting\n";
        assert_eq!(md, expected);
    }

    #[test]
    fn markdown_without_summary_omits_those_sections() {
        let md = to_markdown(&meeting(), &segments(), None);
        assert!(!md.contains("## Summary"));
        assert!(!md.contains("## Action Items"));
        assert!(!md.contains("## Key Decisions"));
        assert!(md.contains("## Transcript"));
    }

    #[test]
    fn markdown_handles_missing_title_and_empty_transcript() {
        let mut m = meeting();
        m.title = None;
        let md = to_markdown(&m, &[], None);
        assert!(md.starts_with("# Meeting: Untitled Meeting\n"));
        assert!(md.contains("_No transcript available._"));
    }

    #[test]
    fn segment_without_speaker_drops_label() {
        let mut segs = segments();
        segs[0].speaker = None;
        let md = to_markdown(&meeting(), &segs, None);
        assert!(md.contains("[00:00:00] Hello everyone\n"));
    }
}
