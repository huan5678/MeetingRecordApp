/**
 * Mock fixtures so the UI renders and is interactive without the Rust backend.
 * Used only by `mockInvoke` in `tauri.ts` (browser / Vitest path). Deterministic.
 */

import type {
  Meeting,
  MediaFile,
  TranscriptRun,
  TranscriptSegment,
  Summary,
} from "@/lib/types";
import type { MeetingDetailDto, AudioDevice } from "@/lib/tauri";

export const MOCK_MEETINGS: Meeting[] = [
  {
    id: "mtg-001",
    title: "Q3 Planning — Product Sync",
    start_time: "2026-06-18T14:00:00Z",
    end_time: "2026-06-18T15:30:00Z",
    duration_seconds: 5400,
    status: "completed",
    tags: ["product", "planning"],
    meeting_type: "team_sync",
    created_at: "2026-06-18T14:00:00Z",
    updated_at: "2026-06-18T15:31:00Z",
  },
  {
    id: "mtg-002",
    title: "1:1 with Alex",
    start_time: "2026-06-17T09:30:00Z",
    end_time: "2026-06-17T10:00:00Z",
    duration_seconds: 1800,
    status: "completed",
    tags: ["1on1"],
    meeting_type: "1on1",
    created_at: "2026-06-17T09:30:00Z",
    updated_at: "2026-06-17T10:01:00Z",
  },
  {
    id: "mtg-003",
    title: "Acme Corp — Discovery Call",
    start_time: "2026-06-16T16:00:00Z",
    end_time: "2026-06-16T16:45:00Z",
    duration_seconds: 2700,
    status: "transcribing",
    tags: ["sales", "acme"],
    meeting_type: "client_call",
    created_at: "2026-06-16T16:00:00Z",
    updated_at: "2026-06-16T16:46:00Z",
  },
  {
    id: "mtg-004",
    title: "Candidate Interview — Backend",
    start_time: "2026-06-15T11:00:00Z",
    end_time: null,
    duration_seconds: null,
    status: "error",
    tags: ["hiring"],
    meeting_type: "interview",
    created_at: "2026-06-15T11:00:00Z",
    updated_at: "2026-06-15T11:05:00Z",
  },
];

const MOCK_MEDIA: MediaFile[] = [
  {
    id: "media-001",
    meeting_id: "mtg-001",
    file_type: "audio",
    file_path: "C:/MeetingRecordApp/recordings/mtg-001.wav",
    file_size_bytes: 103_809_024,
    format: "wav",
    duration_seconds: 5400,
    created_at: "2026-06-18T15:30:00Z",
  },
];

const MOCK_SEGMENTS: TranscriptSegment[] = [
  {
    id: "seg-1",
    meeting_id: "mtg-001",
    segment_index: 0,
    start_time_ms: 0,
    end_time_ms: 4200,
    text: "Welcome everyone. Let's kick off the Q3 planning sync.",
    speaker: "Speaker A",
    confidence: 0.97,
    language: "en",
    created_at: "2026-06-18T15:30:10Z",
  },
  {
    id: "seg-2",
    meeting_id: "mtg-001",
    segment_index: 1,
    start_time_ms: 4200,
    end_time_ms: 9800,
    text: "I'll start with the roadmap. We have three big themes this quarter.",
    speaker: "Speaker B",
    confidence: 0.95,
    language: "en",
    created_at: "2026-06-18T15:30:11Z",
  },
  {
    id: "seg-3",
    meeting_id: "mtg-001",
    segment_index: 2,
    start_time_ms: 9800,
    end_time_ms: 16500,
    text: "First, ship the audio capture pipeline on Windows. That's the top risk.",
    speaker: "Speaker B",
    confidence: 0.93,
    language: "en",
    created_at: "2026-06-18T15:30:12Z",
  },
  {
    id: "seg-4",
    meeting_id: "mtg-001",
    segment_index: 3,
    start_time_ms: 16500,
    end_time_ms: 22000,
    text: "Agreed. Let's make sure the WASAPI loopback spike is done first.",
    speaker: "Speaker A",
    confidence: 0.96,
    language: "en",
    created_at: "2026-06-18T15:30:13Z",
  },
];

const MOCK_SUMMARY: Summary = {
  id: "sum-001",
  meeting_id: "mtg-001",
  summary_type: "auto",
  content:
    "## Overview\n\nThe team aligned on three Q3 themes, with the Windows audio " +
    "capture pipeline identified as the top risk to de-risk first via a WASAPI " +
    "loopback spike.\n\n## Key Points\n\n- Roadmap centers on three themes for the quarter.\n" +
    "- Audio capture on Windows is the highest-priority risk.\n- A Phase 0 spike will " +
    "validate system-audio + mic mixing before further work.",
  action_items: [
    {
      task: "Complete the WASAPI loopback + mic mixing spike",
      owner: "Speaker B",
      deadline: "2026-06-25",
      done: false,
    },
    {
      task: "Draft the Q3 roadmap doc with the three themes",
      owner: "Speaker A",
      deadline: "2026-06-22",
      done: true,
    },
  ],
  key_decisions: [
    {
      decision: "De-risk Windows audio capture first via a Phase 0 spike",
      context: "Highest technical risk for v1.0",
    },
    {
      decision: "Focus the quarter on three roadmap themes",
      context: null,
    },
  ],
  prompt_used: null,
  ai_provider: "ollama",
  ai_model: "llama3.1",
  tokens_used: 4231,
  created_at: "2026-06-18T15:31:00Z",
};

const MOCK_RUNS: TranscriptRun[] = [
  {
    id: "run-001",
    meeting_id: "mtg-001",
    engine: "gemini",
    model: "gemini-3.5-flash",
    language: "zh-TW",
    created_at: "2026-06-18T15:31:00Z",
    segment_count: MOCK_SEGMENTS.length,
  },
];

export const MOCK_AUDIO_DEVICES: AudioDevice[] = [
  { id: "mic-default", name: "Default Microphone", kind: "input", isDefault: true },
  { id: "mic-usb", name: "Blue Yeti USB", kind: "input", isDefault: false },
  {
    id: "out-default",
    name: "Speakers (Realtek)",
    kind: "output",
    isDefault: true,
  },
];

/** Build a detail payload for a given meeting id (only mtg-001 has rich data). */
export function MOCK_DETAIL(id: string): MeetingDetailDto {
  const meeting =
    MOCK_MEETINGS.find((m) => m.id === id) ?? MOCK_MEETINGS[0];
  const isRich = meeting.id === "mtg-001";
  return {
    meeting,
    media: isRich ? MOCK_MEDIA : [],
    segments: isRich ? MOCK_SEGMENTS : [],
    runs: isRich ? MOCK_RUNS : [],
    summaries: isRich ? [MOCK_SUMMARY] : [],
  };
}
