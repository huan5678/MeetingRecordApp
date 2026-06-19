-- MeetingRecordApp — initial schema (v1.0)
-- Source of truth: docs/PRD.md §4.3. Keep in sync with src/models.rs.

PRAGMA foreign_keys = ON;

-- Core meeting record
CREATE TABLE IF NOT EXISTS meetings (
    id TEXT PRIMARY KEY,          -- UUID
    title TEXT,                   -- Auto-detected or user-set
    start_time DATETIME NOT NULL,
    end_time DATETIME,
    duration_seconds INTEGER,
    status TEXT DEFAULT 'recording', -- recording, transcribing, completed, error
    tags TEXT,                    -- JSON array of tags
    meeting_type TEXT,            -- 1on1, team_sync, client_call, interview, other
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Audio/video files
CREATE TABLE IF NOT EXISTS media_files (
    id TEXT PRIMARY KEY,
    meeting_id TEXT REFERENCES meetings(id),
    file_type TEXT NOT NULL,      -- audio, video
    file_path TEXT NOT NULL,
    file_size_bytes INTEGER,
    format TEXT,                  -- wav, mp3, mp4, webm
    duration_seconds INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Transcript segments
CREATE TABLE IF NOT EXISTS transcript_segments (
    id TEXT PRIMARY KEY,
    meeting_id TEXT REFERENCES meetings(id),
    segment_index INTEGER NOT NULL,
    start_time_ms INTEGER NOT NULL,
    end_time_ms INTEGER NOT NULL,
    text TEXT NOT NULL,
    speaker TEXT,                 -- Speaker label if diarization enabled
    confidence REAL,
    language TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- AI summary
CREATE TABLE IF NOT EXISTS summaries (
    id TEXT PRIMARY KEY,
    meeting_id TEXT REFERENCES meetings(id),
    summary_type TEXT NOT NULL,   -- auto, custom, template
    content TEXT NOT NULL,        -- Markdown formatted summary
    action_items TEXT,            -- JSON array of {owner, task, deadline}
    key_decisions TEXT,           -- JSON array of decisions
    prompt_used TEXT,             -- The prompt that generated this summary
    ai_provider TEXT,             -- ollama, openai, claude, gemini
    ai_model TEXT,                -- specific model used
    tokens_used INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Settings
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Helpful lookup indexes (foreign-key navigation + history ordering).
CREATE INDEX IF NOT EXISTS idx_media_files_meeting ON media_files(meeting_id);
CREATE INDEX IF NOT EXISTS idx_segments_meeting ON transcript_segments(meeting_id);
CREATE INDEX IF NOT EXISTS idx_summaries_meeting ON summaries(meeting_id);
CREATE INDEX IF NOT EXISTS idx_meetings_start_time ON meetings(start_time);

-- Full-text search index
CREATE VIRTUAL TABLE IF NOT EXISTS transcript_fts USING fts5(
    text, speaker, language,
    content='transcript_segments',
    content_rowid='rowid'
);

-- FTS5 external-content sync triggers (REQUIRED — keep transcript_fts in sync)
CREATE TRIGGER IF NOT EXISTS transcript_segments_ai AFTER INSERT ON transcript_segments BEGIN
    INSERT INTO transcript_fts(rowid, text, speaker, language)
    VALUES (new.rowid, new.text, new.speaker, new.language);
END;
CREATE TRIGGER IF NOT EXISTS transcript_segments_ad AFTER DELETE ON transcript_segments BEGIN
    INSERT INTO transcript_fts(transcript_fts, rowid, text, speaker, language)
    VALUES ('delete', old.rowid, old.text, old.speaker, old.language);
END;
CREATE TRIGGER IF NOT EXISTS transcript_segments_au AFTER UPDATE ON transcript_segments BEGIN
    INSERT INTO transcript_fts(transcript_fts, rowid, text, speaker, language)
    VALUES ('delete', old.rowid, old.text, old.speaker, old.language);
    INSERT INTO transcript_fts(rowid, text, speaker, language)
    VALUES (new.rowid, new.text, new.speaker, new.language);
END;
