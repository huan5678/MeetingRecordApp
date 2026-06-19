//! FTS5 keyword search over `transcript_fts`.
//!
//! `transcript_fts` is an **external-content** FTS5 table whose `rowid` mirrors
//! `transcript_segments.rowid` (kept in sync by the triggers in
//! `001_initial.sql`). We join back to `transcript_segments` so each hit
//! carries the owning meeting, timing and speaker — enough for the UI to render
//! a result and jump to that moment. `snippet()` gives a highlighted excerpt.
//!
//! User input is treated as a bag of terms with prefix matching, never spliced
//! raw into the MATCH expression: a stray `"` or `*` in an FTS5 query is a
//! syntax error, so [`build_match_query`] quotes each whitespace-delimited
//! token and appends `*` for prefix search.

use rusqlite::OptionalExtension;

use crate::storage::{Database, Result};

/// One transcript hit from a full-text search.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SearchHit {
    /// `transcript_segments.id` of the matching segment.
    pub segment_id: String,
    /// The meeting the segment belongs to (for navigation / grouping).
    pub meeting_id: String,
    /// Title of the owning meeting, if set (denormalized for the results list).
    pub meeting_title: Option<String>,
    pub segment_index: i64,
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    /// Full segment text.
    pub text: String,
    pub speaker: Option<String>,
    /// An excerpt with the matched terms wrapped in `[` … `]`.
    pub snippet: String,
}

/// Turn free-form user input into a safe FTS5 MATCH expression.
///
/// Each whitespace-delimited token is double-quoted (so FTS5 special chars are
/// literal) and suffixed with `*` for prefix matching; tokens are ANDed
/// implicitly by FTS5. Returns `None` when the input has no usable tokens, in
/// which case the caller should short-circuit to an empty result set rather
/// than run a query that errors.
pub fn build_match_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        // A double-quote inside a token would break our own quoting; drop it.
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// Search all transcripts for `query`, returning up to `limit` hits ranked by
/// FTS5 relevance (`bm25`, best first).
pub fn search_transcripts(db: &Database, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let match_expr = match build_match_query(query) {
        Some(q) => q,
        None => return Ok(Vec::new()),
    };

    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT ts.id, ts.meeting_id, m.title, ts.segment_index,
                ts.start_time_ms, ts.end_time_ms, ts.text, ts.speaker,
                snippet(transcript_fts, 0, '[', ']', '…', 12) AS snip
         FROM transcript_fts
         JOIN transcript_segments ts ON ts.rowid = transcript_fts.rowid
         LEFT JOIN meetings m ON m.id = ts.meeting_id
         WHERE transcript_fts MATCH ?1
         ORDER BY bm25(transcript_fts)
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(rusqlite::params![match_expr, limit as i64], |row| {
        Ok(SearchHit {
            segment_id: row.get(0)?,
            meeting_id: row.get(1)?,
            meeting_title: row.get(2)?,
            segment_index: row.get(3)?,
            start_time_ms: row.get(4)?,
            end_time_ms: row.get(5)?,
            text: row.get(6)?,
            speaker: row.get(7)?,
            snippet: row.get(8)?,
        })
    })?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Search within a single meeting's transcript. Same ranking, scoped by
/// `meeting_id`.
pub fn search_transcripts_in_meeting(
    db: &Database,
    meeting_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>> {
    let match_expr = match build_match_query(query) {
        Some(q) => q,
        None => return Ok(Vec::new()),
    };

    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT ts.id, ts.meeting_id, m.title, ts.segment_index,
                ts.start_time_ms, ts.end_time_ms, ts.text, ts.speaker,
                snippet(transcript_fts, 0, '[', ']', '…', 12) AS snip
         FROM transcript_fts
         JOIN transcript_segments ts ON ts.rowid = transcript_fts.rowid
         LEFT JOIN meetings m ON m.id = ts.meeting_id
         WHERE transcript_fts MATCH ?1 AND ts.meeting_id = ?2
         ORDER BY bm25(transcript_fts)
         LIMIT ?3",
    )?;

    let rows = stmt.query_map(
        rusqlite::params![match_expr, meeting_id, limit as i64],
        |row| {
            Ok(SearchHit {
                segment_id: row.get(0)?,
                meeting_id: row.get(1)?,
                meeting_title: row.get(2)?,
                segment_index: row.get(3)?,
                start_time_ms: row.get(4)?,
                end_time_ms: row.get(5)?,
                text: row.get(6)?,
                speaker: row.get(7)?,
                snippet: row.get(8)?,
            })
        },
    )?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Count distinct meetings that contain at least one matching segment. Cheap
/// hook for a "found in N meetings" header without materializing every hit.
pub fn count_matching_meetings(db: &Database, query: &str) -> Result<i64> {
    let match_expr = match build_match_query(query) {
        Some(q) => q,
        None => return Ok(0),
    };
    let conn = db.conn();
    let n = conn
        .query_row(
            "SELECT COUNT(DISTINCT ts.meeting_id)
             FROM transcript_fts
             JOIN transcript_segments ts ON ts.rowid = transcript_fts.rowid
             WHERE transcript_fts MATCH ?1",
            rusqlite::params![match_expr],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(n.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Meeting, MeetingStatus, MeetingType, TranscriptSegment};

    fn meeting(id: &str, title: &str) -> Meeting {
        Meeting {
            id: id.into(),
            title: Some(title.into()),
            start_time: "2026-06-18 14:00:00".into(),
            end_time: None,
            duration_seconds: None,
            status: MeetingStatus::Completed,
            tags: vec![],
            meeting_type: Some(MeetingType::TeamSync),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn segment(id: &str, meeting_id: &str, idx: i64, text: &str, speaker: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: id.into(),
            meeting_id: meeting_id.into(),
            segment_index: idx,
            start_time_ms: idx * 1000,
            end_time_ms: idx * 1000 + 900,
            text: text.into(),
            speaker: Some(speaker.into()),
            confidence: Some(0.9),
            language: Some("en".into()),
            created_at: String::new(),
        }
    }

    fn seeded_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.insert_meeting(&meeting("m1", "Budget review")).unwrap();
        db.insert_meeting(&meeting("m2", "Hiring sync")).unwrap();
        db.insert_transcript_segment(&segment(
            "s1",
            "m1",
            0,
            "We need to finalize the quarterly budget today",
            "Alice",
        ))
        .unwrap();
        db.insert_transcript_segment(&segment(
            "s2",
            "m1",
            1,
            "The marketing budget is too high",
            "Bob",
        ))
        .unwrap();
        db.insert_transcript_segment(&segment(
            "s3",
            "m2",
            0,
            "Let us discuss the hiring pipeline",
            "Carol",
        ))
        .unwrap();
        db
    }

    #[test]
    fn build_match_query_quotes_and_prefixes() {
        assert_eq!(build_match_query("budget").as_deref(), Some("\"budget\"*"));
        assert_eq!(
            build_match_query("marketing budget").as_deref(),
            Some("\"marketing\"* \"budget\"*")
        );
    }

    #[test]
    fn build_match_query_rejects_empty_and_strips_quotes() {
        assert!(build_match_query("").is_none());
        assert!(build_match_query("   ").is_none());
        assert!(build_match_query("\"\"").is_none());
        // A token that is purely quotes collapses to nothing → no query.
        // A normal token with an embedded quote is sanitized, not a syntax error.
        assert_eq!(build_match_query("bud\"get").as_deref(), Some("\"budget\"*"));
    }

    #[test]
    fn search_returns_hits_across_meetings() {
        let db = seeded_db();
        let hits = search_transcripts(&db, "budget", 10).unwrap();
        // "budget" appears in s1 and s2 (both meeting m1).
        assert_eq!(hits.len(), 2);
        let ids: Vec<&str> = hits.iter().map(|h| h.segment_id.as_str()).collect();
        assert!(ids.contains(&"s1"));
        assert!(ids.contains(&"s2"));
        // Hits carry the denormalized meeting title.
        assert!(hits.iter().all(|h| h.meeting_title.as_deref() == Some("Budget review")));
        // Snippet highlights the matched term.
        assert!(hits.iter().any(|h| h.snippet.contains("[budget]") || h.snippet.contains("[Budget]")));
    }

    #[test]
    fn search_no_match_is_empty() {
        let db = seeded_db();
        let hits = search_transcripts(&db, "kubernetes", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn search_prefix_matching_works() {
        let db = seeded_db();
        // "hir" should prefix-match "hiring".
        let hits = search_transcripts(&db, "hir", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].segment_id, "s3");
        assert_eq!(hits[0].meeting_id, "m2");
    }

    #[test]
    fn search_respects_limit() {
        let db = seeded_db();
        let hits = search_transcripts(&db, "budget", 1).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn empty_query_returns_no_hits_without_error() {
        let db = seeded_db();
        assert!(search_transcripts(&db, "   ", 10).unwrap().is_empty());
    }

    #[test]
    fn scoped_search_filters_by_meeting() {
        let db = seeded_db();
        // "budget" exists only in m1; scoping to m2 yields nothing.
        let in_m1 = search_transcripts_in_meeting(&db, "m1", "budget", 10).unwrap();
        assert_eq!(in_m1.len(), 2);
        let in_m2 = search_transcripts_in_meeting(&db, "m2", "budget", 10).unwrap();
        assert!(in_m2.is_empty());
    }

    #[test]
    fn count_matching_meetings_is_distinct() {
        let db = seeded_db();
        // "budget" hits two segments but both are in one meeting.
        assert_eq!(count_matching_meetings(&db, "budget").unwrap(), 1);
        // A term present in both meetings would count 2; "the" appears in
        // m1 (s2) and m2 (s3).
        assert_eq!(count_matching_meetings(&db, "the").unwrap(), 2);
        assert_eq!(count_matching_meetings(&db, "").unwrap(), 0);
    }

    #[test]
    fn deleting_segment_removes_it_from_search() {
        let db = seeded_db();
        assert_eq!(search_transcripts(&db, "budget", 10).unwrap().len(), 2);
        // Deleting the whole meeting fires the FTS delete trigger.
        db.delete_meeting("m1").unwrap();
        assert!(search_transcripts(&db, "budget", 10).unwrap().is_empty());
    }
}
