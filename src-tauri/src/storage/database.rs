//! SQLite operations + migration runner (`rusqlite`, bundled).
//!
//! [`Database`] owns a single `rusqlite::Connection`. v1.0 is a desktop app
//! with a serialized command surface, so a single connection guarded by the
//! caller (Tauri state holds it behind a `Mutex`) is sufficient; we do not pull
//! in an async pool. All row types come from [`crate::models`] — this module
//! only translates between them and SQL.
//!
//! The schema lives in `migrations/001_initial.sql` and is embedded at compile
//! time via [`include_str!`] so the binary is self-contained (no migration
//! files shipped alongside the executable). Re-running the migration is safe:
//! every statement is `CREATE ... IF NOT EXISTS`.

use rusqlite::{Connection, OptionalExtension, Row};

use crate::models::{
    AiProviderKind, MediaFile, MediaFileType, Meeting, MeetingStatus, MeetingType, Settings,
    Summary, SummaryType, TranscriptRun, TranscriptSegment,
};
use crate::storage::{Result, StorageError};

/// The embedded v1.0 schema (PRD §4.3), including the FTS5 virtual table and
/// the REQUIRED external-content sync triggers.
const MIGRATION_001: &str = include_str!("../../migrations/001_initial.sql");

/// Migration 002: versioned transcripts (`transcript_runs` + the `run_id` link).
const MIGRATION_002: &str = include_str!("../../migrations/002_transcript_runs.sql");

/// A handle to the application's SQLite database.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (creating if necessary) the database at `path` and apply migrations.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    /// Open an in-memory database (used by tests). Each instance is isolated.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::from_connection(conn)
    }

    /// Wrap an existing connection: enable foreign keys and run migrations.
    fn from_connection(conn: Connection) -> Result<Self> {
        // Foreign keys are per-connection and OFF by default in SQLite; the
        // delete-cascade behaviour we rely on needs this turned on.
        conn.pragma_update(None, "foreign_keys", true)?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }

    /// Apply the embedded migrations. Idempotent: the SQL is all `IF NOT
    /// EXISTS`, and the one column-add is guarded by a `PRAGMA table_info`
    /// check (SQLite has no `ADD COLUMN IF NOT EXISTS`). Order matters: the
    /// `run_id` column must exist before 002 indexes it.
    pub fn run_migrations(&self) -> Result<()> {
        self.conn.execute_batch(MIGRATION_001)?;
        self.ensure_column("transcript_segments", "run_id", "TEXT")?;
        self.conn.execute_batch(MIGRATION_002)?;
        Ok(())
    }

    /// Add `column` to `table` if it isn't already present. Used for additive
    /// migrations on databases created by an earlier schema version.
    fn ensure_column(&self, table: &str, column: &str, decl_type: &str) -> Result<()> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let exists = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .any(|name| name == column);
        if !exists {
            self.conn
                .execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl_type}"), [])?;
        }
        Ok(())
    }

    /// Borrow the underlying connection (used by [`crate::storage::search`]).
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    // -- meetings -----------------------------------------------------------

    /// Insert a meeting row. `created_at` / `updated_at` are left to the DB
    /// defaults when empty; otherwise the provided values are used verbatim.
    pub fn insert_meeting(&self, m: &Meeting) -> Result<()> {
        let tags = serde_json::to_string(&m.tags)?;
        self.conn.execute(
            "INSERT INTO meetings
                (id, title, start_time, end_time, duration_seconds, status,
                 tags, meeting_type, created_at, updated_at)
             VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                 COALESCE(NULLIF(?9, ''), CURRENT_TIMESTAMP),
                 COALESCE(NULLIF(?10, ''), CURRENT_TIMESTAMP))",
            rusqlite::params![
                m.id,
                m.title,
                m.start_time,
                m.end_time,
                m.duration_seconds,
                m.status.as_db_str(),
                tags,
                m.meeting_type.map(|t| t.as_db_str()),
                m.created_at,
                m.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Fetch a meeting by id. `Ok(None)` when there's no such row.
    pub fn get_meeting(&self, id: &str) -> Result<Option<Meeting>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, title, start_time, end_time, duration_seconds, status,
                        tags, meeting_type, created_at, updated_at
                 FROM meetings WHERE id = ?1",
                [id],
                row_to_meeting,
            )
            .optional()?;
        row.transpose()
    }

    /// All meetings, newest first (by `start_time`). Drives the history list.
    pub fn list_meetings(&self) -> Result<Vec<Meeting>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, start_time, end_time, duration_seconds, status,
                    tags, meeting_type, created_at, updated_at
             FROM meetings ORDER BY start_time DESC, created_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_meeting)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r??);
        }
        Ok(out)
    }

    /// Update the mutable fields of a meeting and bump `updated_at`.
    pub fn update_meeting(&self, m: &Meeting) -> Result<()> {
        let tags = serde_json::to_string(&m.tags)?;
        let n = self.conn.execute(
            "UPDATE meetings SET
                title = ?2, start_time = ?3, end_time = ?4,
                duration_seconds = ?5, status = ?6, tags = ?7,
                meeting_type = ?8, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            rusqlite::params![
                m.id,
                m.title,
                m.start_time,
                m.end_time,
                m.duration_seconds,
                m.status.as_db_str(),
                tags,
                m.meeting_type.map(|t| t.as_db_str()),
            ],
        )?;
        if n == 0 {
            return Err(StorageError::NotFound {
                entity: "meeting",
                id: m.id.clone(),
            });
        }
        Ok(())
    }

    /// Set just the lifecycle status (the common transition during the
    /// record → transcribe → complete pipeline).
    pub fn set_meeting_status(&self, id: &str, status: MeetingStatus) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE meetings SET status = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            rusqlite::params![id, status.as_db_str()],
        )?;
        if n == 0 {
            return Err(StorageError::NotFound {
                entity: "meeting",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Delete a meeting and every child row (media files, transcript segments,
    /// summaries). The schema's `REFERENCES` lack `ON DELETE CASCADE`, so we
    /// delete children explicitly inside one transaction. Deleting segments
    /// fires the FTS `AFTER DELETE` trigger, keeping `transcript_fts` in sync.
    pub fn delete_meeting(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM transcript_segments WHERE meeting_id = ?1",
            [id],
        )?;
        tx.execute("DELETE FROM media_files WHERE meeting_id = ?1", [id])?;
        tx.execute("DELETE FROM summaries WHERE meeting_id = ?1", [id])?;
        let n = tx.execute("DELETE FROM meetings WHERE id = ?1", [id])?;
        tx.commit()?;
        if n == 0 {
            return Err(StorageError::NotFound {
                entity: "meeting",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    // -- media_files --------------------------------------------------------

    pub fn insert_media_file(&self, f: &MediaFile) -> Result<()> {
        self.conn.execute(
            "INSERT INTO media_files
                (id, meeting_id, file_type, file_path, file_size_bytes,
                 format, duration_seconds, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                     COALESCE(NULLIF(?8, ''), CURRENT_TIMESTAMP))",
            rusqlite::params![
                f.id,
                f.meeting_id,
                media_file_type_str(f.file_type),
                f.file_path,
                f.file_size_bytes,
                f.format,
                f.duration_seconds,
                f.created_at,
            ],
        )?;
        Ok(())
    }

    /// All media files for a meeting, in insertion order.
    pub fn list_media_files(&self, meeting_id: &str) -> Result<Vec<MediaFile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, meeting_id, file_type, file_path, file_size_bytes,
                    format, duration_seconds, created_at
             FROM media_files WHERE meeting_id = ?1 ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt.query_map([meeting_id], row_to_media_file)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r??);
        }
        Ok(out)
    }

    // -- transcript_segments ------------------------------------------------

    /// Insert one segment. The `AFTER INSERT` trigger mirrors it into
    /// `transcript_fts`.
    pub fn insert_transcript_segment(&self, s: &TranscriptSegment) -> Result<()> {
        self.conn.execute(
            "INSERT INTO transcript_segments
                (id, meeting_id, segment_index, start_time_ms, end_time_ms,
                 text, speaker, confidence, language, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                     COALESCE(NULLIF(?10, ''), CURRENT_TIMESTAMP))",
            rusqlite::params![
                s.id,
                s.meeting_id,
                s.segment_index,
                s.start_time_ms,
                s.end_time_ms,
                s.text,
                s.speaker,
                s.confidence,
                s.language,
                s.created_at,
            ],
        )?;
        Ok(())
    }

    /// Insert many segments in one transaction (the transcription pipeline
    /// writes a whole meeting's worth at once).
    /// Insert a batch of segments, all belonging to the same transcription run
    /// (`run_id`). `run_id` is `None` only for legacy/test inserts that predate
    /// versioned transcripts; new code always passes one.
    pub fn insert_transcript_segments(
        &self,
        segments: &[TranscriptSegment],
        run_id: Option<&str>,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO transcript_segments
                    (id, meeting_id, segment_index, start_time_ms, end_time_ms,
                     text, speaker, confidence, language, created_at, run_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                         COALESCE(NULLIF(?10, ''), CURRENT_TIMESTAMP), ?11)",
            )?;
            for s in segments {
                stmt.execute(rusqlite::params![
                    s.id,
                    s.meeting_id,
                    s.segment_index,
                    s.start_time_ms,
                    s.end_time_ms,
                    s.text,
                    s.speaker,
                    s.confidence,
                    s.language,
                    s.created_at,
                    run_id,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Segments of a meeting's **latest** transcription run, ordered by the
    /// timeline. This is the default view (detail page, export, summary input).
    /// Falls back to all segments for legacy meetings that have no run rows.
    pub fn list_transcript_segments(&self, meeting_id: &str) -> Result<Vec<TranscriptSegment>> {
        match self.latest_run_id(meeting_id)? {
            Some(run_id) => self.list_transcript_segments_for_run(&run_id),
            None => self.list_legacy_segments(meeting_id),
        }
    }

    /// All segments for `run_id`, ordered by the timeline.
    pub fn list_transcript_segments_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<TranscriptSegment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, meeting_id, segment_index, start_time_ms, end_time_ms,
                    text, speaker, confidence, language, created_at
             FROM transcript_segments WHERE run_id = ?1
             ORDER BY segment_index ASC, start_time_ms ASC",
        )?;
        let rows = stmt.query_map([run_id], row_to_segment)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// All segments for a meeting regardless of run (used as the legacy
    /// fallback when a meeting predates `transcript_runs`).
    fn list_legacy_segments(&self, meeting_id: &str) -> Result<Vec<TranscriptSegment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, meeting_id, segment_index, start_time_ms, end_time_ms,
                    text, speaker, confidence, language, created_at
             FROM transcript_segments WHERE meeting_id = ?1
             ORDER BY segment_index ASC, start_time_ms ASC",
        )?;
        let rows = stmt.query_map([meeting_id], row_to_segment)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // -- transcript runs ----------------------------------------------------

    /// Record a transcription run. `created_at` defaults to now when empty.
    pub fn insert_transcript_run(&self, r: &TranscriptRun) -> Result<()> {
        self.conn.execute(
            "INSERT INTO transcript_runs (id, meeting_id, engine, model, language, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, COALESCE(NULLIF(?6, ''), CURRENT_TIMESTAMP))",
            rusqlite::params![r.id, r.meeting_id, r.engine, r.model, r.language, r.created_at],
        )?;
        Ok(())
    }

    /// All runs for a meeting, newest first, each with its segment count.
    pub fn list_transcript_runs(&self, meeting_id: &str) -> Result<Vec<TranscriptRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.meeting_id, r.engine, r.model, r.language, r.created_at,
                    (SELECT COUNT(*) FROM transcript_segments s WHERE s.run_id = r.id)
             FROM transcript_runs r WHERE r.meeting_id = ?1
             ORDER BY r.created_at DESC, r.rowid DESC",
        )?;
        let rows = stmt.query_map([meeting_id], row_to_run)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// The most recent run id for a meeting, or `None` if it has no runs.
    fn latest_run_id(&self, meeting_id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM transcript_runs WHERE meeting_id = ?1
                 ORDER BY created_at DESC, rowid DESC LIMIT 1",
                [meeting_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?)
    }

    /// Delete a run and the segments it produced (the `AFTER DELETE` trigger
    /// keeps the FTS index in sync).
    pub fn delete_transcript_run(&self, run_id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM transcript_segments WHERE run_id = ?1", [run_id])?;
        tx.execute("DELETE FROM transcript_runs WHERE id = ?1", [run_id])?;
        tx.commit()?;
        Ok(())
    }

    /// Delete a single summary (the user can prune old regenerations).
    pub fn delete_summary(&self, summary_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM summaries WHERE id = ?1", [summary_id])?;
        Ok(())
    }

    /// Update a segment's text/speaker (transcript editing, post-diarization
    /// speaker relabelling). The `AFTER UPDATE` trigger re-syncs the FTS row.
    pub fn update_transcript_segment(&self, s: &TranscriptSegment) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE transcript_segments SET
                segment_index = ?2, start_time_ms = ?3, end_time_ms = ?4,
                text = ?5, speaker = ?6, confidence = ?7, language = ?8
             WHERE id = ?1",
            rusqlite::params![
                s.id,
                s.segment_index,
                s.start_time_ms,
                s.end_time_ms,
                s.text,
                s.speaker,
                s.confidence,
                s.language,
            ],
        )?;
        if n == 0 {
            return Err(StorageError::NotFound {
                entity: "transcript_segment",
                id: s.id.clone(),
            });
        }
        Ok(())
    }

    // -- summaries ----------------------------------------------------------

    pub fn insert_summary(&self, s: &Summary) -> Result<()> {
        let action_items = serde_json::to_string(&s.action_items)?;
        let key_decisions = serde_json::to_string(&s.key_decisions)?;
        self.conn.execute(
            "INSERT INTO summaries
                (id, meeting_id, summary_type, content, action_items,
                 key_decisions, prompt_used, ai_provider, ai_model,
                 tokens_used, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                     COALESCE(NULLIF(?11, ''), CURRENT_TIMESTAMP))",
            rusqlite::params![
                s.id,
                s.meeting_id,
                summary_type_str(s.summary_type),
                s.content,
                action_items,
                key_decisions,
                s.prompt_used,
                s.ai_provider.map(|p| p.as_db_str()),
                s.ai_model,
                s.tokens_used,
                s.created_at,
            ],
        )?;
        Ok(())
    }

    /// All summaries for a meeting, newest first (a meeting can have several:
    /// the auto summary plus user regenerations).
    pub fn list_summaries(&self, meeting_id: &str) -> Result<Vec<Summary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, meeting_id, summary_type, content, action_items,
                    key_decisions, prompt_used, ai_provider, ai_model,
                    tokens_used, created_at
             FROM summaries WHERE meeting_id = ?1 ORDER BY created_at DESC, rowid DESC",
        )?;
        let rows = stmt.query_map([meeting_id], row_to_summary)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r??);
        }
        Ok(out)
    }

    // -- settings -----------------------------------------------------------

    /// Upsert a setting (key/value), bumping `updated_at`.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value, updated_at)
             VALUES (?1, ?2, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value, updated_at = CURRENT_TIMESTAMP",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    /// Fetch a single setting value, or `None` if unset.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let v = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(v)
    }

    /// All settings rows (used to hydrate the settings store on startup).
    pub fn list_settings(&self) -> Result<Vec<Settings>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value, updated_at FROM settings ORDER BY key ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(Settings {
                key: row.get(0)?,
                value: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Delete a setting. Returns `true` if a row was removed.
    pub fn delete_setting(&self, key: &str) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(n > 0)
    }
}

// ---------------------------------------------------------------------------
// Enum <-> column-string helpers. `models` ships `as_db_str`/`from_db_str` for
// most enums; `MediaFileType` and `SummaryType` only have serde reprs, so we
// keep their string mapping (and parsing) local to the storage layer.
// ---------------------------------------------------------------------------

fn media_file_type_str(t: MediaFileType) -> &'static str {
    match t {
        MediaFileType::Audio => "audio",
        MediaFileType::Video => "video",
    }
}

fn media_file_type_from_str(s: &str) -> Result<MediaFileType> {
    match s {
        "audio" => Ok(MediaFileType::Audio),
        "video" => Ok(MediaFileType::Video),
        other => Err(StorageError::InvalidEnum {
            column: "media_files.file_type",
            value: other.to_string(),
        }),
    }
}

fn summary_type_str(t: SummaryType) -> &'static str {
    match t {
        SummaryType::Auto => "auto",
        SummaryType::Custom => "custom",
        SummaryType::Template => "template",
    }
}

fn summary_type_from_str(s: &str) -> Result<SummaryType> {
    match s {
        "auto" => Ok(SummaryType::Auto),
        "custom" => Ok(SummaryType::Custom),
        "template" => Ok(SummaryType::Template),
        other => Err(StorageError::InvalidEnum {
            column: "summaries.summary_type",
            value: other.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Row mappers. Each returns `rusqlite::Result<Result<T>>` for the rows that can
// fail JSON/enum decoding: the outer result is rusqlite's column access, the
// inner is our `StorageError`. Mappers that can't fail decoding return a plain
// `rusqlite::Result<T>`.
// ---------------------------------------------------------------------------

fn row_to_meeting(row: &Row) -> rusqlite::Result<Result<Meeting>> {
    let status_str: String = row.get(5)?;
    let tags_str: Option<String> = row.get(6)?;
    let type_str: Option<String> = row.get(7)?;

    Ok((|| {
        let status = MeetingStatus::from_db_str(&status_str).ok_or_else(|| {
            StorageError::InvalidEnum {
                column: "meetings.status",
                value: status_str.clone(),
            }
        })?;
        let meeting_type = match type_str.as_deref() {
            None => None,
            Some(s) => Some(MeetingType::from_db_str(s).ok_or_else(|| {
                StorageError::InvalidEnum {
                    column: "meetings.meeting_type",
                    value: s.to_string(),
                }
            })?),
        };
        let tags: Vec<String> = match tags_str.as_deref() {
            None | Some("") => Vec::new(),
            Some(json) => serde_json::from_str(json)?,
        };
        Ok(Meeting {
            id: row.get(0)?,
            title: row.get(1)?,
            start_time: row.get(2)?,
            end_time: row.get(3)?,
            duration_seconds: row.get(4)?,
            status,
            tags,
            meeting_type,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    })())
}

fn row_to_media_file(row: &Row) -> rusqlite::Result<Result<MediaFile>> {
    let type_str: String = row.get(2)?;
    Ok((|| {
        Ok(MediaFile {
            id: row.get(0)?,
            meeting_id: row.get(1)?,
            file_type: media_file_type_from_str(&type_str)?,
            file_path: row.get(3)?,
            file_size_bytes: row.get(4)?,
            format: row.get(5)?,
            duration_seconds: row.get(6)?,
            created_at: row.get(7)?,
        })
    })())
}

fn row_to_segment(row: &Row) -> rusqlite::Result<TranscriptSegment> {
    Ok(TranscriptSegment {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        segment_index: row.get(2)?,
        start_time_ms: row.get(3)?,
        end_time_ms: row.get(4)?,
        text: row.get(5)?,
        speaker: row.get(6)?,
        confidence: row.get(7)?,
        language: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn row_to_run(row: &Row) -> rusqlite::Result<TranscriptRun> {
    Ok(TranscriptRun {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        engine: row.get(2)?,
        model: row.get(3)?,
        language: row.get(4)?,
        created_at: row.get(5)?,
        segment_count: row.get(6)?,
    })
}

fn row_to_summary(row: &Row) -> rusqlite::Result<Result<Summary>> {
    let type_str: String = row.get(2)?;
    let action_items_str: Option<String> = row.get(4)?;
    let key_decisions_str: Option<String> = row.get(5)?;
    let provider_str: Option<String> = row.get(7)?;
    Ok((|| {
        let action_items = match action_items_str.as_deref() {
            None | Some("") => Vec::new(),
            Some(json) => serde_json::from_str(json)?,
        };
        let key_decisions = match key_decisions_str.as_deref() {
            None | Some("") => Vec::new(),
            Some(json) => serde_json::from_str(json)?,
        };
        let ai_provider = match provider_str.as_deref() {
            None => None,
            Some(s) => Some(AiProviderKind::from_db_str(s).ok_or_else(|| {
                StorageError::InvalidEnum {
                    column: "summaries.ai_provider",
                    value: s.to_string(),
                }
            })?),
        };
        Ok(Summary {
            id: row.get(0)?,
            meeting_id: row.get(1)?,
            summary_type: summary_type_from_str(&type_str)?,
            content: row.get(3)?,
            action_items,
            key_decisions,
            prompt_used: row.get(6)?,
            ai_provider,
            ai_model: row.get(8)?,
            tokens_used: row.get(9)?,
            created_at: row.get(10)?,
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ActionItem, KeyDecision};

    fn new_db() -> Database {
        Database::open_in_memory().expect("open in-memory db")
    }

    fn sample_meeting(id: &str) -> Meeting {
        Meeting {
            id: id.to_string(),
            title: Some("Weekly sync".into()),
            start_time: "2026-06-18 14:00:00".into(),
            end_time: Some("2026-06-18 15:00:00".into()),
            duration_seconds: Some(3600),
            status: MeetingStatus::Completed,
            tags: vec!["project-x".into(), "weekly".into()],
            meeting_type: Some(MeetingType::TeamSync),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn sample_segment(id: &str, meeting_id: &str, idx: i64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: id.to_string(),
            meeting_id: meeting_id.to_string(),
            segment_index: idx,
            start_time_ms: idx * 1000,
            end_time_ms: idx * 1000 + 900,
            text: text.to_string(),
            speaker: Some("Speaker A".into()),
            confidence: Some(0.95),
            language: Some("en".into()),
            created_at: String::new(),
        }
    }

    #[test]
    fn migration_creates_all_tables_and_fts() {
        let db = new_db();
        let names: Vec<String> = {
            let mut stmt = db
                .conn()
                .prepare("SELECT name FROM sqlite_master WHERE type IN ('table','view') ORDER BY name")
                .unwrap();
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            rows
        };
        for expected in [
            "meetings",
            "media_files",
            "transcript_segments",
            "summaries",
            "settings",
            "transcript_fts",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "missing table {expected}; got {names:?}"
            );
        }
    }

    #[test]
    fn run_migrations_is_idempotent() {
        let db = new_db();
        // A second run must not error (every statement is IF NOT EXISTS).
        db.run_migrations().expect("second migration run");
    }

    #[test]
    fn insert_and_get_meeting_roundtrip() {
        let db = new_db();
        let m = sample_meeting("m1");
        db.insert_meeting(&m).unwrap();

        let got = db.get_meeting("m1").unwrap().expect("meeting present");
        assert_eq!(got.id, "m1");
        assert_eq!(got.title.as_deref(), Some("Weekly sync"));
        assert_eq!(got.status, MeetingStatus::Completed);
        assert_eq!(got.meeting_type, Some(MeetingType::TeamSync));
        assert_eq!(got.tags, vec!["project-x".to_string(), "weekly".to_string()]);
        assert_eq!(got.duration_seconds, Some(3600));
        // DB-defaulted timestamps are populated.
        assert!(!got.created_at.is_empty());
        assert!(!got.updated_at.is_empty());
    }

    #[test]
    fn get_missing_meeting_returns_none() {
        let db = new_db();
        assert!(db.get_meeting("nope").unwrap().is_none());
    }

    #[test]
    fn list_meetings_orders_newest_first() {
        let db = new_db();
        let mut a = sample_meeting("a");
        a.start_time = "2026-01-01 09:00:00".into();
        let mut b = sample_meeting("b");
        b.start_time = "2026-06-01 09:00:00".into();
        db.insert_meeting(&a).unwrap();
        db.insert_meeting(&b).unwrap();

        let list = db.list_meetings().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "b", "newest start_time first");
        assert_eq!(list[1].id, "a");
    }

    #[test]
    fn update_meeting_changes_fields() {
        let db = new_db();
        let mut m = sample_meeting("m1");
        db.insert_meeting(&m).unwrap();

        m.title = Some("Renamed".into());
        m.status = MeetingStatus::Error;
        m.tags = vec!["urgent".into()];
        db.update_meeting(&m).unwrap();

        let got = db.get_meeting("m1").unwrap().unwrap();
        assert_eq!(got.title.as_deref(), Some("Renamed"));
        assert_eq!(got.status, MeetingStatus::Error);
        assert_eq!(got.tags, vec!["urgent".to_string()]);
    }

    #[test]
    fn update_missing_meeting_is_not_found() {
        let db = new_db();
        let m = sample_meeting("ghost");
        let err = db.update_meeting(&m).unwrap_err();
        assert!(matches!(err, StorageError::NotFound { entity: "meeting", .. }));
    }

    #[test]
    fn set_meeting_status_updates_only_status() {
        let db = new_db();
        let m = sample_meeting("m1");
        db.insert_meeting(&m).unwrap();
        db.set_meeting_status("m1", MeetingStatus::Transcribing).unwrap();
        let got = db.get_meeting("m1").unwrap().unwrap();
        assert_eq!(got.status, MeetingStatus::Transcribing);
        assert_eq!(got.title.as_deref(), Some("Weekly sync"));
    }

    #[test]
    fn media_file_insert_and_list() {
        let db = new_db();
        db.insert_meeting(&sample_meeting("m1")).unwrap();
        let f = MediaFile {
            id: "f1".into(),
            meeting_id: "m1".into(),
            file_type: MediaFileType::Audio,
            file_path: "/recordings/m1/mix.wav".into(),
            file_size_bytes: Some(1_048_576),
            format: Some("wav".into()),
            duration_seconds: Some(3600),
            created_at: String::new(),
        };
        db.insert_media_file(&f).unwrap();

        let list = db.list_media_files("m1").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].file_type, MediaFileType::Audio);
        assert_eq!(list[0].file_path, "/recordings/m1/mix.wav");
        assert_eq!(list[0].format.as_deref(), Some("wav"));
    }

    #[test]
    fn insert_segment_creates_fts_row_via_trigger() {
        let db = new_db();
        db.insert_meeting(&sample_meeting("m1")).unwrap();
        db.insert_transcript_segment(&sample_segment("s1", "m1", 0, "hello world"))
            .unwrap();

        // The AFTER INSERT trigger must have mirrored the row into the FTS table.
        let fts_count: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM transcript_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_count, 1, "FTS trigger should have inserted a row");
    }

    #[test]
    fn batch_insert_segments_and_list_ordered() {
        let db = new_db();
        db.insert_meeting(&sample_meeting("m1")).unwrap();
        let segs = vec![
            sample_segment("s2", "m1", 1, "second"),
            sample_segment("s1", "m1", 0, "first"),
        ];
        // No run id → legacy fallback returns every segment for the meeting.
        db.insert_transcript_segments(&segs, None).unwrap();

        let list = db.list_transcript_segments("m1").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].segment_index, 0, "ordered by segment_index");
        assert_eq!(list[1].segment_index, 1);
    }

    #[test]
    fn transcript_runs_are_versioned_and_default_to_latest() {
        let db = new_db();
        db.insert_meeting(&sample_meeting("m1")).unwrap();

        let run1 = TranscriptRun {
            id: "r1".into(),
            meeting_id: "m1".into(),
            engine: "whisper".into(),
            model: "belle-turbo-zh".into(),
            language: Some("zh".into()),
            created_at: "2026-06-23 10:00:00".into(),
            segment_count: 0,
        };
        let run2 = TranscriptRun {
            id: "r2".into(),
            engine: "gemini".into(),
            model: "gemini-3.5-flash".into(),
            created_at: "2026-06-23 11:00:00".into(),
            ..run1.clone()
        };
        db.insert_transcript_run(&run1).unwrap();
        db.insert_transcript_run(&run2).unwrap();
        db.insert_transcript_segments(&[sample_segment("a", "m1", 0, "old run")], Some("r1"))
            .unwrap();
        db.insert_transcript_segments(
            &[
                sample_segment("b", "m1", 0, "new run 1"),
                sample_segment("c", "m1", 1, "new run 2"),
            ],
            Some("r2"),
        )
        .unwrap();

        // Default view = latest run (r2).
        let latest = db.list_transcript_segments("m1").unwrap();
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0].text, "new run 1");

        // Explicit older run still retrievable.
        let old = db.list_transcript_segments_for_run("r1").unwrap();
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].text, "old run");

        // Runs listed newest-first with counts.
        let runs = db.list_transcript_runs("m1").unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, "r2");
        assert_eq!(runs[0].segment_count, 2);
        assert_eq!(runs[1].segment_count, 1);

        // Deleting a run drops its segments; default falls back to the other.
        db.delete_transcript_run("r2").unwrap();
        assert_eq!(db.list_transcript_runs("m1").unwrap().len(), 1);
        assert_eq!(db.list_transcript_segments("m1").unwrap()[0].text, "old run");
    }

    #[test]
    fn update_segment_resyncs_fts() {
        let db = new_db();
        db.insert_meeting(&sample_meeting("m1")).unwrap();
        let mut s = sample_segment("s1", "m1", 0, "alpha");
        db.insert_transcript_segment(&s).unwrap();

        // Before: search for the new word finds nothing.
        let before = crate::storage::search::search_transcripts(&db, "betaword", 10).unwrap();
        assert!(before.is_empty());

        s.text = "betaword here".into();
        db.update_transcript_segment(&s).unwrap();

        let after = crate::storage::search::search_transcripts(&db, "betaword", 10).unwrap();
        assert_eq!(after.len(), 1, "update trigger should re-sync FTS");
        assert_eq!(after[0].segment_id, "s1");
    }

    #[test]
    fn summary_insert_and_list_roundtrip() {
        let db = new_db();
        db.insert_meeting(&sample_meeting("m1")).unwrap();
        let s = Summary {
            id: "sum1".into(),
            meeting_id: "m1".into(),
            summary_type: SummaryType::Auto,
            content: "## Notes\nWe shipped.".into(),
            action_items: vec![ActionItem {
                task: "Write tests".into(),
                owner: Some("Alex".into()),
                deadline: Some("2026-06-20".into()),
                done: false,
            }],
            key_decisions: vec![KeyDecision {
                decision: "Use SQLite".into(),
                context: None,
            }],
            prompt_used: Some("summarize".into()),
            ai_provider: Some(AiProviderKind::Ollama),
            ai_model: Some("llama3".into()),
            tokens_used: Some(1234),
            created_at: String::new(),
        };
        db.insert_summary(&s).unwrap();

        let list = db.list_summaries("m1").unwrap();
        assert_eq!(list.len(), 1);
        let got = &list[0];
        assert_eq!(got.summary_type, SummaryType::Auto);
        assert_eq!(got.ai_provider, Some(AiProviderKind::Ollama));
        assert_eq!(got.action_items.len(), 1);
        assert_eq!(got.action_items[0].owner.as_deref(), Some("Alex"));
        assert_eq!(got.key_decisions[0].decision, "Use SQLite");
        assert_eq!(got.tokens_used, Some(1234));
    }

    #[test]
    fn settings_upsert_get_list_delete() {
        let db = new_db();
        db.set_setting("theme", "dark").unwrap();
        assert_eq!(db.get_setting("theme").unwrap().as_deref(), Some("dark"));

        // Upsert overwrites.
        db.set_setting("theme", "light").unwrap();
        assert_eq!(db.get_setting("theme").unwrap().as_deref(), Some("light"));

        db.set_setting("model", "small").unwrap();
        let all = db.list_settings().unwrap();
        assert_eq!(all.len(), 2);
        // Ordered by key.
        assert_eq!(all[0].key, "model");
        assert_eq!(all[1].key, "theme");

        assert!(db.delete_setting("model").unwrap());
        assert!(!db.delete_setting("model").unwrap());
        assert!(db.get_setting("model").unwrap().is_none());
    }

    #[test]
    fn delete_meeting_cascades_children_and_fts() {
        let db = new_db();
        db.insert_meeting(&sample_meeting("m1")).unwrap();
        db.insert_media_file(&MediaFile {
            id: "f1".into(),
            meeting_id: "m1".into(),
            file_type: MediaFileType::Audio,
            file_path: "/r/m1.wav".into(),
            file_size_bytes: Some(10),
            format: Some("wav".into()),
            duration_seconds: Some(1),
            created_at: String::new(),
        })
        .unwrap();
        db.insert_transcript_segment(&sample_segment("s1", "m1", 0, "cascade target"))
            .unwrap();
        db.insert_summary(&Summary {
            id: "sum1".into(),
            meeting_id: "m1".into(),
            summary_type: SummaryType::Auto,
            content: "x".into(),
            action_items: vec![],
            key_decisions: vec![],
            prompt_used: None,
            ai_provider: None,
            ai_model: None,
            tokens_used: None,
            created_at: String::new(),
        })
        .unwrap();

        db.delete_meeting("m1").unwrap();

        assert!(db.get_meeting("m1").unwrap().is_none());
        assert!(db.list_media_files("m1").unwrap().is_empty());
        assert!(db.list_transcript_segments("m1").unwrap().is_empty());
        assert!(db.list_summaries("m1").unwrap().is_empty());
        // The AFTER DELETE trigger should have cleared the FTS row too.
        let fts_count: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM transcript_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_count, 0, "FTS rows should be gone after cascade delete");
    }

    #[test]
    fn delete_missing_meeting_is_not_found() {
        let db = new_db();
        let err = db.delete_meeting("ghost").unwrap_err();
        assert!(matches!(err, StorageError::NotFound { entity: "meeting", .. }));
    }
}
