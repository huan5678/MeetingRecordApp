/**
 * TypeScript mirrors of the Rust domain types in `src-tauri/src/models.rs`.
 *
 * These are the shapes that cross the Tauri IPC boundary. Field names match the
 * serde output exactly (serde uses the Rust field names as-is here — no
 * `rename_all` on the structs — and the enums serialize to their `*_db_str`
 * lowercase string, captured by the union types below).
 *
 * Keep in lock-step with models.rs. The string unions reuse the constants so a
 * value can only ever be one the backend knows.
 */

import type {
  MeetingStatus,
  MeetingType,
  AiProvider,
} from "@/lib/constants";

export type { MeetingStatus, MeetingType, AiProvider };

export type MediaFileType = "audio" | "video";
export type SummaryKind = "auto" | "custom" | "template";

/** Maps `models::Meeting`. */
export interface Meeting {
  id: string;
  title: string | null;
  start_time: string;
  end_time: string | null;
  duration_seconds: number | null;
  status: MeetingStatus;
  tags: string[];
  meeting_type: MeetingType | null;
  created_at: string;
  updated_at: string;
}

/** Maps `models::MediaFile`. */
export interface MediaFile {
  id: string;
  meeting_id: string;
  file_type: MediaFileType;
  file_path: string;
  file_size_bytes: number | null;
  format: string | null;
  duration_seconds: number | null;
  created_at: string;
}

/** Maps `models::TranscriptSegment`. */
export interface TranscriptSegment {
  id: string;
  meeting_id: string;
  segment_index: number;
  start_time_ms: number;
  end_time_ms: number;
  text: string;
  speaker: string | null;
  confidence: number | null;
  language: string | null;
  created_at: string;
}

/** Maps `models::TranscriptRun` — one transcription run (engine/model) of a meeting. */
export interface TranscriptRun {
  id: string;
  meeting_id: string;
  engine: string;
  model: string;
  language: string | null;
  created_at: string;
  segment_count: number;
}

/** Maps `models::ActionItem` (element of `summaries.action_items`). */
export interface ActionItem {
  task: string;
  owner: string | null;
  deadline: string | null;
  done: boolean;
}

/** Maps `models::KeyDecision` (element of `summaries.key_decisions`). */
export interface KeyDecision {
  decision: string;
  context: string | null;
}

/** Maps `models::Summary`. */
export interface Summary {
  id: string;
  meeting_id: string;
  summary_type: SummaryKind;
  content: string;
  action_items: ActionItem[];
  key_decisions: KeyDecision[];
  prompt_used: string | null;
  ai_provider: AiProvider | null;
  ai_model: string | null;
  tokens_used: number | null;
  created_at: string;
}
