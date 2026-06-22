-- MeetingRecordApp — migration 002: versioned transcripts.
-- A meeting can be transcribed more than once (different engine/model), and we
-- keep every result so the user can compare/view them separately. Each run
-- groups the segments it produced (transcript_segments.run_id). Summaries are
-- already multi-row per meeting, so they need no schema change.
--
-- The run_id column on transcript_segments is added by an idempotent ALTER in
-- Rust (run_migrations) — SQLite has no "ADD COLUMN IF NOT EXISTS".

CREATE TABLE IF NOT EXISTS transcript_runs (
    id TEXT PRIMARY KEY,          -- UUID
    meeting_id TEXT REFERENCES meetings(id),
    engine TEXT NOT NULL,         -- gemini | whisper
    model TEXT NOT NULL,          -- e.g. gemini-3.5-flash, belle-turbo-zh
    language TEXT,                -- forced language or NULL (auto)
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_runs_meeting ON transcript_runs(meeting_id);
CREATE INDEX IF NOT EXISTS idx_segments_run ON transcript_segments(run_id);
