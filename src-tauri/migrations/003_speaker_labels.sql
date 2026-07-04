-- Migration 003: speaker_labels
--
-- Maps a meeting's raw diarization label (e.g. "Speaker 1") to a real display
-- name. Filled by the manual label-once UI (source='manual') and, later, by
-- identity providers (Teams UIA / Meet captions) and voiceprint memory.
--
-- Additive + idempotent: there is no version table (see
-- database.rs::run_migrations), so this must stay CREATE ... IF NOT EXISTS.
CREATE TABLE IF NOT EXISTS speaker_labels (
    meeting_id   TEXT NOT NULL,
    raw_label    TEXT NOT NULL,
    display_name TEXT NOT NULL,
    source       TEXT NOT NULL DEFAULT 'manual',
    PRIMARY KEY (meeting_id, raw_label)
);
