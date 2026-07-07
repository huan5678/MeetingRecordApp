-- Phase 3: cross-meeting voiceprint memory.
-- Additive & idempotent (no version table; see database.rs::run_migrations).

-- Per-meeting, per-cluster speaker embeddings, computed at transcription time
-- (diarize feature only). Staging so a later manual label can promote a
-- cluster's voiceprint into the named library without re-processing audio.
CREATE TABLE IF NOT EXISTS meeting_cluster_embeddings (
    meeting_id TEXT NOT NULL,
    raw_label  TEXT NOT NULL,      -- diarization label, e.g. "Speaker 1"
    embedding  BLOB NOT NULL,      -- f32 little-endian, `dim` floats
    dim        INTEGER NOT NULL,
    PRIMARY KEY (meeting_id, raw_label)
);

-- The cross-meeting memory: one enrolled embedding per display name.
CREATE TABLE IF NOT EXISTS speaker_voiceprints (
    name       TEXT NOT NULL PRIMARY KEY,
    embedding  BLOB NOT NULL,
    dim        INTEGER NOT NULL
);
