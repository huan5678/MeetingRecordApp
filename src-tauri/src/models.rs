//! Shared domain types — the contract between every backend module and the
//! frontend. These mirror the SQLite schema in `migrations/001_initial.sql`
//! (PRD §4.3). Keep the two in lock-step.
//!
//! Conventions:
//! - Every struct derives `Serialize`/`Deserialize` so it crosses the Tauri
//!   IPC boundary and is also storable as JSON where the schema uses JSON
//!   columns (e.g. `meetings.tags`, `summaries.action_items`).
//! - Timestamps are stored as SQLite `DATETIME` text (ISO-8601 /
//!   `CURRENT_TIMESTAMP`); we keep them as `String` here so the storage layer
//!   doesn't force a chrono dependency on every consumer. Durations are plain
//!   integers (seconds or milliseconds, as named).
//! - Enums serialize to the exact lowercase string the DB stores, via
//!   `#[serde(rename_all = "snake_case")]` plus explicit renames where the DB
//!   value isn't snake_case (e.g. `1on1`).

use serde::{Deserialize, Serialize};

/// Lifecycle of a meeting record. Stored in `meetings.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingStatus {
    /// Audio is being captured.
    Recording,
    /// Recording finished; transcription/diarization in progress.
    Transcribing,
    /// Transcript (and optionally summary) available.
    Completed,
    /// A pipeline step failed; see logs.
    Error,
}

impl MeetingStatus {
    /// The exact string persisted in the `status` column.
    pub fn as_db_str(self) -> &'static str {
        match self {
            MeetingStatus::Recording => "recording",
            MeetingStatus::Transcribing => "transcribing",
            MeetingStatus::Completed => "completed",
            MeetingStatus::Error => "error",
        }
    }

    /// Parse the DB string back into the enum.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "recording" => Some(MeetingStatus::Recording),
            "transcribing" => Some(MeetingStatus::Transcribing),
            "completed" => Some(MeetingStatus::Completed),
            "error" => Some(MeetingStatus::Error),
            _ => None,
        }
    }
}

impl Default for MeetingStatus {
    fn default() -> Self {
        MeetingStatus::Recording
    }
}

/// Meeting category, drives which summary template is used (PRD §4.6).
/// Stored in `meetings.meeting_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingType {
    /// 1:1 meeting. DB value is `1on1` (not a valid Rust ident, hence rename).
    #[serde(rename = "1on1")]
    OneOnOne,
    TeamSync,
    ClientCall,
    Interview,
    Other,
}

impl MeetingType {
    pub fn as_db_str(self) -> &'static str {
        match self {
            MeetingType::OneOnOne => "1on1",
            MeetingType::TeamSync => "team_sync",
            MeetingType::ClientCall => "client_call",
            MeetingType::Interview => "interview",
            MeetingType::Other => "other",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "1on1" => Some(MeetingType::OneOnOne),
            "team_sync" => Some(MeetingType::TeamSync),
            "client_call" => Some(MeetingType::ClientCall),
            "interview" => Some(MeetingType::Interview),
            "other" => Some(MeetingType::Other),
            _ => None,
        }
    }
}

impl Default for MeetingType {
    fn default() -> Self {
        MeetingType::Other
    }
}

/// Kind of media on disk. Stored in `media_files.file_type`. v1.0 only ever
/// writes `Audio`; `Video` exists for the v1.1 screen-recording path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaFileType {
    Audio,
    Video,
}

/// How a summary was produced. Stored in `summaries.summary_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryType {
    /// Generated automatically after transcription with the default template.
    Auto,
    /// Generated from a user-supplied custom prompt.
    Custom,
    /// Generated from a named template (1:1, team sync, ...).
    Template,
}

/// Which LLM backend produced a summary. Stored in `summaries.ai_provider`.
/// Ollama is the local, default, no-API-key provider (PRD §4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderKind {
    Ollama,
    OpenAi,
    Claude,
    Gemini,
}

impl AiProviderKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            AiProviderKind::Ollama => "ollama",
            AiProviderKind::OpenAi => "openai",
            AiProviderKind::Claude => "claude",
            AiProviderKind::Gemini => "gemini",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "ollama" => Some(AiProviderKind::Ollama),
            "openai" => Some(AiProviderKind::OpenAi),
            "claude" => Some(AiProviderKind::Claude),
            "gemini" => Some(AiProviderKind::Gemini),
            _ => None,
        }
    }

    /// Cloud providers require an API key and incur cost; Ollama does not.
    pub fn is_cloud(self) -> bool {
        !matches!(self, AiProviderKind::Ollama)
    }
}

impl Default for AiProviderKind {
    fn default() -> Self {
        AiProviderKind::Ollama
    }
}

// ---------------------------------------------------------------------------
// Row types — one struct per table.
// ---------------------------------------------------------------------------

/// A meeting record. Maps to table `meetings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub title: Option<String>,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_seconds: Option<i64>,
    pub status: MeetingStatus,
    /// JSON array of free-form tags. Deserialized from the `tags` TEXT column.
    #[serde(default)]
    pub tags: Vec<String>,
    pub meeting_type: Option<MeetingType>,
    pub created_at: String,
    pub updated_at: String,
}

/// An on-disk media file belonging to a meeting. Maps to `media_files`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub id: String,
    pub meeting_id: String,
    pub file_type: MediaFileType,
    pub file_path: String,
    pub file_size_bytes: Option<i64>,
    /// Container/codec extension: wav, mp3, mp4, webm.
    pub format: Option<String>,
    pub duration_seconds: Option<i64>,
    pub created_at: String,
}

/// A single transcript line with timing and optional speaker label.
/// Maps to `transcript_segments` (and is indexed by `transcript_fts`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub meeting_id: String,
    pub segment_index: i64,
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub text: String,
    /// Speaker label written by the diarization step (PRD §4.5); `None` when
    /// diarization is disabled or unavailable.
    pub speaker: Option<String>,
    pub confidence: Option<f64>,
    /// BCP-47-ish language tag detected by whisper (e.g. "zh", "en", "ja").
    pub language: Option<String>,
    pub created_at: String,
}

/// One transcription run for a meeting (a meeting can be transcribed more than
/// once with different engines/models — we keep every result). Maps to
/// `transcript_runs`; its segments are `transcript_segments` with this `id` as
/// their `run_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptRun {
    pub id: String,
    pub meeting_id: String,
    /// Engine that produced this run: "gemini" | "whisper".
    pub engine: String,
    /// Model id (e.g. "gemini-3.5-flash", "belle-turbo-zh").
    pub model: String,
    /// Forced language ("zh"/…) or `None` for auto-detect.
    pub language: Option<String>,
    pub created_at: String,
    /// Number of segments in this run. Derived at query time (not a column).
    #[serde(default)]
    pub segment_count: i64,
}

/// An AI-generated summary for a meeting. Maps to `summaries`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub meeting_id: String,
    pub summary_type: SummaryType,
    /// Markdown body.
    pub content: String,
    /// Parsed from the `action_items` JSON column.
    #[serde(default)]
    pub action_items: Vec<ActionItem>,
    /// Parsed from the `key_decisions` JSON column.
    #[serde(default)]
    pub key_decisions: Vec<KeyDecision>,
    pub prompt_used: Option<String>,
    pub ai_provider: Option<AiProviderKind>,
    pub ai_model: Option<String>,
    pub tokens_used: Option<i64>,
    pub created_at: String,
}

/// One actionable follow-up extracted from the meeting. Serialized as an
/// element of the `summaries.action_items` JSON array (PRD §4.3: {owner, task,
/// deadline}). `done` is UI-side state, defaulted for backward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionItem {
    pub task: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub deadline: Option<String>,
    #[serde(default)]
    pub done: bool,
}

/// A key decision made during the meeting. Serialized as an element of the
/// `summaries.key_decisions` JSON array.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyDecision {
    pub decision: String,
    #[serde(default)]
    pub context: Option<String>,
}

/// A single key/value setting row. Maps to `settings`. Values are stored as
/// text (often JSON-encoded by the caller).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meeting_status_db_roundtrip() {
        for s in [
            MeetingStatus::Recording,
            MeetingStatus::Transcribing,
            MeetingStatus::Completed,
            MeetingStatus::Error,
        ] {
            assert_eq!(MeetingStatus::from_db_str(s.as_db_str()), Some(s));
        }
        assert_eq!(MeetingStatus::from_db_str("bogus"), None);
        assert_eq!(MeetingStatus::default(), MeetingStatus::Recording);
    }

    #[test]
    fn meeting_type_db_roundtrip_including_1on1() {
        for t in [
            MeetingType::OneOnOne,
            MeetingType::TeamSync,
            MeetingType::ClientCall,
            MeetingType::Interview,
            MeetingType::Other,
        ] {
            assert_eq!(MeetingType::from_db_str(t.as_db_str()), Some(t));
        }
        assert_eq!(MeetingType::OneOnOne.as_db_str(), "1on1");
        assert_eq!(MeetingType::from_db_str("1on1"), Some(MeetingType::OneOnOne));
    }

    #[test]
    fn meeting_status_serializes_to_db_string() {
        // serde representation must equal the DB string so IPC + storage agree.
        assert_eq!(
            serde_json::to_string(&MeetingStatus::Transcribing).unwrap(),
            "\"transcribing\""
        );
    }

    #[test]
    fn meeting_type_one_on_one_serializes_as_1on1() {
        assert_eq!(
            serde_json::to_string(&MeetingType::OneOnOne).unwrap(),
            "\"1on1\""
        );
    }

    #[test]
    fn ai_provider_cloud_classification() {
        assert!(!AiProviderKind::Ollama.is_cloud());
        assert!(AiProviderKind::OpenAi.is_cloud());
        assert!(AiProviderKind::Claude.is_cloud());
        assert!(AiProviderKind::Gemini.is_cloud());
        assert_eq!(AiProviderKind::default(), AiProviderKind::Ollama);
    }

    #[test]
    fn ai_provider_db_roundtrip() {
        for p in [
            AiProviderKind::Ollama,
            AiProviderKind::OpenAi,
            AiProviderKind::Claude,
            AiProviderKind::Gemini,
        ] {
            assert_eq!(AiProviderKind::from_db_str(p.as_db_str()), Some(p));
        }
    }

    #[test]
    fn action_item_deserializes_with_defaults() {
        // Minimal JSON (only `task`) must fill the optional fields.
        let item: ActionItem = serde_json::from_str(r#"{"task":"ship v1.0"}"#).unwrap();
        assert_eq!(item.task, "ship v1.0");
        assert_eq!(item.owner, None);
        assert_eq!(item.deadline, None);
        assert!(!item.done);
    }

    #[test]
    fn key_decision_roundtrip() {
        let d = KeyDecision {
            decision: "Adopt Tauri 2".into(),
            context: Some("smaller binary".into()),
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: KeyDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }
}
