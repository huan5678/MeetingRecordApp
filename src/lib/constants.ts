/**
 * App-wide constants. Pure data, no side effects — safe to import anywhere
 * (including the Zustand stores and Vitest tests).
 *
 * These mirror the v1.0 PRD decisions (Windows-only, audio-only, Ollama-default)
 * and the enum string values defined on the Rust side in
 * `src-tauri/src/models.rs`. Keep the string literals in lock-step with the DB
 * `*_db_str` values so IPC + storage agree.
 */

/** Routes for the main window (hash-router-free; we keep a tiny enum in store). */
export const VIEWS = {
  Meetings: "meetings",
  Detail: "detail",
  Settings: "settings",
} as const;
export type View = (typeof VIEWS)[keyof typeof VIEWS];

/** Recording lifecycle as surfaced to the UI (a superset of Meeting.status). */
export const RECORDING_STATES = {
  Idle: "idle",
  Recording: "recording",
  Paused: "paused",
  Stopping: "stopping",
} as const;
export type RecordingState =
  (typeof RECORDING_STATES)[keyof typeof RECORDING_STATES];

/** `meetings.status` values — must equal `MeetingStatus::as_db_str` (models.rs). */
export const MEETING_STATUS = {
  Recording: "recording",
  Transcribing: "transcribing",
  Completed: "completed",
  Error: "error",
} as const;
export type MeetingStatus =
  (typeof MEETING_STATUS)[keyof typeof MEETING_STATUS];

/** `meetings.meeting_type` values — must equal `MeetingType::as_db_str`. */
export const MEETING_TYPES = {
  OneOnOne: "1on1",
  TeamSync: "team_sync",
  ClientCall: "client_call",
  Interview: "interview",
  Other: "other",
} as const;
export type MeetingType = (typeof MEETING_TYPES)[keyof typeof MEETING_TYPES];

/** Human labels for meeting types (drives the summary template select). */
export const MEETING_TYPE_LABELS: Record<MeetingType, string> = {
  "1on1": "1:1",
  team_sync: "Team Sync",
  client_call: "Client Call",
  interview: "Interview",
  other: "General",
};

/** `summaries.ai_provider` values — must equal `AiProviderKind::as_db_str`. */
export const AI_PROVIDERS = {
  Ollama: "ollama",
  OpenAi: "openai",
  Claude: "claude",
  Gemini: "gemini",
} as const;
export type AiProvider = (typeof AI_PROVIDERS)[keyof typeof AI_PROVIDERS];

/** Ollama is the local, no-API-key default (PRD §4.6). */
export const DEFAULT_AI_PROVIDER: AiProvider = AI_PROVIDERS.Ollama;

/** Provider display metadata for the AI settings tab. */
export interface ProviderMeta {
  id: AiProvider;
  label: string;
  /** true for cloud providers that need an API key + incur cost. */
  cloud: boolean;
  /** Default models offered in the model select. */
  models: string[];
}

export const PROVIDER_META: Record<AiProvider, ProviderMeta> = {
  ollama: {
    id: "ollama",
    label: "Ollama (Local, default)",
    cloud: false,
    models: ["llama3.1", "qwen2.5", "gemma2", "mistral"],
  },
  openai: {
    id: "openai",
    label: "OpenAI",
    cloud: true,
    models: ["gpt-4o", "gpt-4o-mini", "gpt-4-turbo"],
  },
  claude: {
    id: "claude",
    label: "Claude",
    cloud: true,
    models: [
      "claude-3-5-sonnet-latest",
      "claude-3-5-haiku-latest",
      "claude-3-opus-latest",
    ],
  },
  gemini: {
    id: "gemini",
    label: "Gemini",
    cloud: true,
    models: ["gemini-1.5-pro", "gemini-1.5-flash"],
  },
};

/**
 * Whisper model sizes (PRD §4.5). Default small/base; medium/large-v3 optional
 * (better Traditional Chinese quality, slower + larger).
 */
export const WHISPER_MODELS = [
  { id: "tiny", label: "Tiny (~75 MB, fastest)" },
  { id: "base", label: "Base (~142 MB, default)" },
  { id: "small", label: "Small (~466 MB, recommended)" },
  { id: "medium", label: "Medium (~1.5 GB, slower)" },
  { id: "large-v3", label: "Large v3 (~3 GB, best, slowest)" },
] as const;
export type WhisperModel = (typeof WHISPER_MODELS)[number]["id"];
export const DEFAULT_WHISPER_MODEL: WhisperModel = "base";

/** Transcription languages whisper supports for our target users (PRD §3.3). */
export const TRANSCRIPTION_LANGUAGES = [
  { id: "auto", label: "Auto-detect" },
  { id: "zh", label: "中文 (Chinese)" },
  { id: "en", label: "English" },
  { id: "ja", label: "日本語 (Japanese)" },
] as const;
export type TranscriptionLanguage =
  (typeof TRANSCRIPTION_LANGUAGES)[number]["id"];

/** Export formats (PRD §4.8). PDF is v1.1/optional, kept out of the v1.0 set. */
export const EXPORT_FORMATS = [
  { id: "markdown", label: "Markdown (.md)", ext: "md" },
  { id: "srt", label: "SubRip (.srt)", ext: "srt" },
  { id: "vtt", label: "WebVTT (.vtt)", ext: "vtt" },
  { id: "json", label: "JSON (.json)", ext: "json" },
] as const;
export type ExportFormat = (typeof EXPORT_FORMATS)[number]["id"];

/**
 * `settings` table keys (PRD §4.3). The backend persists these as text rows;
 * the frontend mirrors them in `settingsStore`. Centralised here so the Rust
 * Integrate phase and the store never drift on a magic string.
 */
export const SETTINGS_KEYS = {
  Theme: "general.theme",
  AutoStart: "general.auto_start",
  MinimizeToTray: "general.minimize_to_tray",
  AutoDetectMeetings: "general.auto_detect_meetings",
  StorageDir: "general.storage_dir",
  MicDeviceId: "audio.mic_device_id",
  SystemAudioEnabled: "audio.system_audio_enabled",
  KeepDualTrack: "audio.keep_dual_track",
  WhisperModel: "transcription.whisper_model",
  Language: "transcription.language",
  DiarizationEnabled: "transcription.diarization_enabled",
  AiProvider: "ai.provider",
  AiModel: "ai.model",
  OllamaEndpoint: "ai.ollama_endpoint",
} as const;

export const DEFAULT_OLLAMA_ENDPOINT = "http://localhost:11434";

/** Theme options for the general settings tab; "system" follows the OS. */
export const THEMES = ["system", "light", "dark"] as const;
export type Theme = (typeof THEMES)[number];
export const DEFAULT_THEME: Theme = "system";

/** Keyboard shortcuts surfaced in settings / tooltips (PRD §3.7 #41). */
export const SHORTCUTS = {
  toggleRecording: "Ctrl+Shift+R",
  stopRecording: "Ctrl+Shift+S",
} as const;
