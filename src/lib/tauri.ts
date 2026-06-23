/**
 * Typed wrappers around the Tauri IPC bridge.
 *
 * Every backend call goes through `call()`, which:
 *   1. forwards to `@tauri-apps/api/core`'s `invoke` when running inside the
 *      Tauri webview, and
 *   2. falls back to in-process MOCK data when running in a plain browser /
 *      Vitest (`isTauri()` is false), so the UI is fully usable without the
 *      Rust backend.
 *
 * COMMAND NAMES (`COMMANDS`) define the contract the Rust `commands.rs` must
 * expose to `tauri::generate_handler!`. They are currently consumed only by the
 * frontend; the Integrate phase wires matching `#[tauri::command]` fns. See the
 * structured-output `notes` for the full list.
 */

import type {
  Meeting,
  MediaFile,
  TranscriptRun,
  TranscriptSegment,
  Summary,
} from "@/lib/types";
import { MOCK_MEETINGS, MOCK_DETAIL, MOCK_AUDIO_DEVICES } from "@/lib/mocks";
import type { ExportFormat } from "@/lib/constants";

/** The snake_case command names the Rust side registers. */
export const COMMANDS = {
  // recording
  startRecording: "start_recording",
  stopRecording: "stop_recording",
  pauseRecording: "pause_recording",
  resumeRecording: "resume_recording",
  getRecordingStatus: "get_recording_status",
  // meetings
  listMeetings: "list_meetings",
  getMeetingDetail: "get_meeting_detail",
  deleteMeeting: "delete_meeting",
  updateMeeting: "update_meeting",
  searchTranscripts: "search_transcripts",
  // transcription
  getTranscriptionStatus: "get_transcription_status",
  retranscribeMeeting: "retranscribe_meeting",
  importAudioMeeting: "import_audio_meeting",
  listTranscriptRuns: "list_transcript_runs",
  getRunSegments: "get_run_segments",
  deleteTranscriptRun: "delete_transcript_run",
  clearTranscripts: "clear_transcripts",
  deleteSummary: "delete_summary",
  updateSegment: "update_segment",
  // summary
  generateSummary: "generate_summary",
  estimateSummaryCost: "estimate_summary_cost",
  // export
  exportMeeting: "export_meeting",
  // settings
  getSettings: "get_settings",
  setSetting: "set_setting",
  listAudioDevices: "list_audio_devices",
  setApiKey: "set_api_key",
  hasApiKey: "has_api_key",
  // storage
  getStorageUsage: "get_storage_usage",
  migrateRecordings: "migrate_recordings",
} as const;

/** True when running inside the Tauri webview (the `__TAURI_INTERNALS__` glue). */
export function isTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    // Tauri 2.x injects this object into the webview's window.
    "__TAURI_INTERNALS__" in window
  );
}

/**
 * Invoke a backend command. When not in Tauri, resolves the registered mock so
 * callers never have to special-case the dev/browser path.
 */
export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(command, args);
  }
  return mockInvoke<T>(command, args);
}

// ---------------------------------------------------------------------------
// Typed command surface — one thin function per backend command.
// ---------------------------------------------------------------------------

export interface RecordingStatusDto {
  state: "idle" | "recording" | "paused" | "stopping";
  meetingId: string | null;
  elapsedSeconds: number;
  /** 0..1 peak level for the mic, for the AudioLevel meter. */
  micLevel: number;
  /** 0..1 peak level for system audio. */
  systemLevel: number;
}

export interface MeetingDetailDto {
  meeting: Meeting;
  media: MediaFile[];
  /** Segments of the latest transcription run (default view). */
  segments: TranscriptSegment[];
  /** Every transcription run, newest first (label by model + time). */
  runs: TranscriptRun[];
  /** Every summary, newest first. */
  summaries: Summary[];
}

/** Per-call transcription overrides for re-running / importing. */
export interface TranscribeOpts {
  engine?: string;
  geminiModel?: string;
  whisperModel?: string;
  language?: string;
}

export interface AudioDevice {
  id: string;
  name: string;
  kind: "input" | "output";
  isDefault: boolean;
}

export interface TranscriptionStatusDto {
  meetingId: string;
  stage: "queued" | "transcribing" | "diarizing" | "done" | "error";
  /** 0..1 */
  progress: number;
  message: string | null;
}

export interface StorageUsageDto {
  totalBytes: number;
  meetingCount: number;
}

export interface MigrateResultDto {
  moved: number;
  skipped: number;
  failed: number;
}

export interface CostEstimateDto {
  provider: string;
  model: string;
  promptTokens: number;
  estimatedUsd: number;
}

export const api = {
  // --- recording ---
  startRecording: (opts: { micDeviceId?: string; systemAudio: boolean }) =>
    call<string>(COMMANDS.startRecording, opts),
  stopRecording: () => call<string>(COMMANDS.stopRecording),
  pauseRecording: () => call<void>(COMMANDS.pauseRecording),
  resumeRecording: () => call<void>(COMMANDS.resumeRecording),
  getRecordingStatus: () =>
    call<RecordingStatusDto>(COMMANDS.getRecordingStatus),

  // --- meetings ---
  listMeetings: () => call<Meeting[]>(COMMANDS.listMeetings),
  getMeetingDetail: (id: string) =>
    call<MeetingDetailDto>(COMMANDS.getMeetingDetail, { id }),
  deleteMeeting: (id: string) => call<void>(COMMANDS.deleteMeeting, { id }),
  updateMeeting: (id: string, patch: Partial<Meeting>) =>
    call<Meeting>(COMMANDS.updateMeeting, { id, patch }),
  searchTranscripts: (query: string) =>
    call<Meeting[]>(COMMANDS.searchTranscripts, { query }),

  // --- transcription ---
  getTranscriptionStatus: (meetingId: string) =>
    call<TranscriptionStatusDto>(COMMANDS.getTranscriptionStatus, {
      meetingId,
    }),
  updateSegment: (segmentId: string, text: string, speaker: string | null) =>
    call<void>(COMMANDS.updateSegment, { segmentId, text, speaker }),
  /** Re-run transcription for an existing meeting; appends a new run. */
  retranscribeMeeting: (meetingId: string, opts: TranscribeOpts = {}) =>
    call<void>(COMMANDS.retranscribeMeeting, { meetingId, ...opts }),
  /** Import an audio file as a new meeting + transcribe it. Returns its id. */
  importAudioMeeting: (
    opts: { filePath: string; title?: string; meetingType?: string } & TranscribeOpts,
  ) => call<string>(COMMANDS.importAudioMeeting, { ...opts }),
  listTranscriptRuns: (meetingId: string) =>
    call<TranscriptRun[]>(COMMANDS.listTranscriptRuns, { meetingId }),
  getRunSegments: (runId: string) =>
    call<TranscriptSegment[]>(COMMANDS.getRunSegments, { runId }),
  deleteTranscriptRun: (runId: string) =>
    call<void>(COMMANDS.deleteTranscriptRun, { runId }),
  /** Clear every transcript run for a meeting at once. */
  clearTranscripts: (meetingId: string) =>
    call<void>(COMMANDS.clearTranscripts, { meetingId }),
  deleteSummary: (summaryId: string) =>
    call<void>(COMMANDS.deleteSummary, { summaryId }),

  // --- summary ---
  generateSummary: (opts: {
    meetingId: string;
    provider: string;
    model: string;
    prompt?: string;
  }) => call<Summary>(COMMANDS.generateSummary, opts),
  estimateSummaryCost: (opts: {
    meetingId: string;
    provider: string;
    model: string;
  }) => call<CostEstimateDto>(COMMANDS.estimateSummaryCost, opts),

  // --- export ---
  exportMeeting: (opts: {
    meetingId: string;
    format: ExportFormat;
    dest?: string;
  }) => call<string>(COMMANDS.exportMeeting, opts),

  // --- settings ---
  getSettings: () => call<Record<string, string>>(COMMANDS.getSettings),
  setSetting: (key: string, value: string) =>
    call<void>(COMMANDS.setSetting, { key, value }),
  listAudioDevices: () => call<AudioDevice[]>(COMMANDS.listAudioDevices),
  setApiKey: (provider: string, key: string) =>
    call<void>(COMMANDS.setApiKey, { provider, key }),
  hasApiKey: (provider: string) =>
    call<boolean>(COMMANDS.hasApiKey, { provider }),

  // --- storage ---
  getStorageUsage: () => call<StorageUsageDto>(COMMANDS.getStorageUsage),
  /** Relocate all existing recordings into the current storage folder. */
  migrateRecordings: () =>
    call<MigrateResultDto>(COMMANDS.migrateRecordings),
};

// ---------------------------------------------------------------------------
// Mock backend — keeps the app runnable without Rust. Deterministic, no timers.
// ---------------------------------------------------------------------------

async function mockInvoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  switch (command) {
    case COMMANDS.listMeetings:
      return MOCK_MEETINGS as unknown as T;
    case COMMANDS.searchTranscripts: {
      const q = String(args?.query ?? "").toLowerCase();
      const hits = MOCK_MEETINGS.filter((m) =>
        (m.title ?? "").toLowerCase().includes(q),
      );
      return hits as unknown as T;
    }
    case COMMANDS.getMeetingDetail:
      return MOCK_DETAIL(String(args?.id ?? "")) as unknown as T;
    case COMMANDS.listAudioDevices:
      return MOCK_AUDIO_DEVICES as unknown as T;
    case COMMANDS.getRecordingStatus:
      return {
        state: "idle",
        meetingId: null,
        elapsedSeconds: 0,
        micLevel: 0,
        systemLevel: 0,
      } as unknown as T;
    case COMMANDS.getStorageUsage:
      return {
        totalBytes: MOCK_MEETINGS.length * 98 * 1024 * 1024,
        meetingCount: MOCK_MEETINGS.length,
      } as unknown as T;
    case COMMANDS.migrateRecordings:
      return { moved: 0, skipped: MOCK_MEETINGS.length, failed: 0 } as unknown as T;
    case COMMANDS.hasApiKey:
      return false as unknown as T;
    case COMMANDS.getSettings:
      return {} as unknown as T;
    case COMMANDS.estimateSummaryCost:
      return {
        provider: String(args?.provider ?? "ollama"),
        model: String(args?.model ?? ""),
        promptTokens: 4200,
        estimatedUsd: String(args?.provider) === "ollama" ? 0 : 0.012,
      } as unknown as T;
    case COMMANDS.listTranscriptRuns:
      return MOCK_DETAIL(String(args?.meetingId ?? "")).runs as unknown as T;
    case COMMANDS.getRunSegments:
      return MOCK_DETAIL("").segments as unknown as T;
    case COMMANDS.startRecording:
    case COMMANDS.stopRecording:
    case COMMANDS.importAudioMeeting:
    case COMMANDS.exportMeeting:
      return "mock-id" as unknown as T;
    // Fire-and-forget commands resolve to undefined in mock mode.
    case COMMANDS.pauseRecording:
    case COMMANDS.resumeRecording:
    case COMMANDS.deleteMeeting:
    case COMMANDS.retranscribeMeeting:
    case COMMANDS.deleteTranscriptRun:
    case COMMANDS.clearTranscripts:
    case COMMANDS.deleteSummary:
    case COMMANDS.updateSegment:
    case COMMANDS.setSetting:
    case COMMANDS.setApiKey:
      return undefined as unknown as T;
    default:
      throw new Error(`mockInvoke: unhandled command "${command}"`);
  }
}
