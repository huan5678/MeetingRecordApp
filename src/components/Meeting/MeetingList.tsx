/**
 * MeetingList — the meeting history view (PRD §3.5). Search box (FTS over
 * transcripts), status badges, tags, and a row click that opens the detail view.
 */

import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useMeetings } from "@/hooks/useMeetings";
import { useRecordingStore } from "@/stores/recordingStore";
import { Button } from "@/components/common/Button";
import { api, isTauri } from "@/lib/tauri";
import { formatDateTime, formatDuration, meetingTitle } from "@/lib/format";
import {
  MEETING_STATUS,
  MEETING_TYPE_LABELS,
  RECORDING_STATES,
  type MeetingStatus,
} from "@/lib/constants";
import type { Meeting } from "@/lib/types";

const STATUS_LABEL: Record<MeetingStatus, string> = {
  [MEETING_STATUS.Recording]: "Rec",
  [MEETING_STATUS.Transcribing]: "Transcribing",
  [MEETING_STATUS.Completed]: "Done",
  [MEETING_STATUS.Error]: "Failed",
};

/** Status as an uppercase tracked label — no colored badges in a mono system. */
function StatusLabel({ status }: { status: MeetingStatus }) {
  const label = STATUS_LABEL[status];
  if (status === MEETING_STATUS.Recording) {
    return (
      <span className="eyebrow flex shrink-0 items-center gap-1.5 self-center text-fg">
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-fg" />
        {label}
      </span>
    );
  }
  const tone =
    status === MEETING_STATUS.Transcribing
      ? "animate-pulse text-fg"
      : status === MEETING_STATUS.Error
        ? "text-fg underline decoration-line-strong underline-offset-4"
        : "text-faint";
  return <span className={`eyebrow shrink-0 self-center ${tone}`}>{label}</span>;
}

export function MeetingList() {
  const { meetings, loading, error, search, refresh, remove } = useMeetings();
  const openMeeting = useRecordingStore((s) => s.openMeeting);
  const recState = useRecordingStore((s) => s.state);
  const [query, setQuery] = useState("");
  const [importing, setImporting] = useState(false);

  // Import an existing audio file as a new meeting and transcribe it (no
  // recording). Opens the new meeting so its progress is visible.
  const importAudio = async () => {
    if (!isTauri()) return;
    const picked = await open({
      multiple: false,
      filters: [
        {
          name: "Audio",
          extensions: ["wav", "mp3", "m4a", "aac", "ogg", "flac", "aiff", "webm"],
        },
      ],
    });
    if (typeof picked !== "string") return;
    setImporting(true);
    try {
      const id = await api.importAudioMeeting({ filePath: picked });
      await refresh();
      openMeeting(id);
    } finally {
      setImporting(false);
    }
  };

  // A recording just finished (→ idle): its duration/status landed in the DB,
  // so refetch the list instead of showing the stale in-progress row.
  const prevRecState = useRef(recState);
  useEffect(() => {
    if (
      prevRecState.current !== RECORDING_STATES.Idle &&
      recState === RECORDING_STATES.Idle
    ) {
      void refresh();
    }
    prevRecState.current = recState;
  }, [recState, refresh]);

  // Transcription finishes on a background worker AFTER the list last refetched,
  // so a row can sit stuck on "transcribing". While any meeting is still
  // recording/transcribing, poll until it settles (then the effect's deps go
  // empty-of-pending and the interval is cleared).
  const hasPending = meetings.some(
    (m) =>
      m.status === MEETING_STATUS.Transcribing ||
      m.status === MEETING_STATUS.Recording,
  );
  useEffect(() => {
    if (!hasPending) return;
    const id = setInterval(() => void refresh(), 2500);
    return () => clearInterval(id);
  }, [hasPending, refresh]);

  return (
    <div className="mx-auto flex h-full w-full max-w-3xl flex-col px-8 pt-8">
      <header className="flex items-end justify-between gap-4 pb-5">
        <div>
          <span className="eyebrow">History</span>
          <h1 className="mt-2 font-display text-2xl font-medium tracking-tight text-fg">
            <span className="num">{meetings.length}</span>{" "}
            {meetings.length === 1 ? "meeting" : "meetings"}
          </h1>
        </div>
        <Button onClick={() => void importAudio()} disabled={importing}>
          {importing ? "匯入中…" : "匯入音檔"}
        </Button>
      </header>

      <div className="flex items-center gap-4 border-y border-line">
        <input
          type="search"
          value={query}
          placeholder="Search transcripts"
          aria-label="Search transcripts"
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void search(query);
          }}
          className="h-12 flex-1 bg-transparent text-sm text-fg placeholder:text-faint focus:outline-none"
        />
        {query && (
          <button
            type="button"
            onClick={() => {
              setQuery("");
              void refresh();
            }}
            className="eyebrow text-faint transition-colors hover:text-fg"
          >
            Clear
          </button>
        )}
        <button
          type="button"
          onClick={() => void search(query)}
          className="eyebrow text-muted transition-colors hover:text-fg"
        >
          Search
        </button>
      </div>

      {error && (
        <p className="num mt-4 text-[13px] text-fg" role="alert">
          {error}
        </p>
      )}

      <div className="-mx-2 flex-1 overflow-auto">
        {loading && meetings.length === 0 ? (
          <p className="px-2 py-6 text-sm text-muted">Loading…</p>
        ) : meetings.length === 0 ? (
          <div className="px-2 py-16">
            <span className="eyebrow">No meetings yet</span>
            <p className="mt-2 text-sm text-muted">
              Press ● Record in the header to capture your first meeting.
            </p>
          </div>
        ) : (
          <ul className="border-b border-line">
            {meetings.map((m, i) => (
              <MeetingRow
                key={m.id}
                meeting={m}
                index={i}
                onOpen={() => openMeeting(m.id)}
                onDelete={() => {
                  const name = meetingTitle(m.title, m.start_time);
                  if (
                    window.confirm(
                      `Delete “${name}”? This removes its recording and transcript and cannot be undone.`,
                    )
                  ) {
                    void remove(m.id);
                  }
                }}
              />
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function MeetingRow({
  meeting,
  index,
  onOpen,
  onDelete,
}: {
  meeting: Meeting;
  index: number;
  onOpen: () => void;
  onDelete: () => void;
}) {
  return (
    <li className="group flex items-stretch border-t border-line">
      <button
        type="button"
        onClick={onOpen}
        className="flex min-w-0 flex-1 items-baseline gap-4 px-2 py-4 text-left transition-colors hover:bg-surface"
      >
        <span className="num w-7 shrink-0 text-[11px] text-faint">
          {String(index + 1).padStart(2, "0")}
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline gap-3">
            <span className="truncate font-display text-[15px] font-medium text-fg">
              {meetingTitle(meeting.title, meeting.start_time)}
            </span>
            {meeting.meeting_type && (
              <span className="eyebrow shrink-0 text-faint">
                {MEETING_TYPE_LABELS[meeting.meeting_type]}
              </span>
            )}
          </span>
          <span className="num mt-1.5 flex items-center gap-2 text-[11px] text-muted">
            {formatDateTime(meeting.start_time)}
            <span className="text-faint">/</span>
            {formatDuration(meeting.duration_seconds)}
            {meeting.tags.length > 0 && (
              <span className="truncate text-faint">
                / {meeting.tags.map((t) => `#${t}`).join(" ")}
              </span>
            )}
          </span>
        </span>
        <StatusLabel status={meeting.status} />
      </button>
      <button
        type="button"
        onClick={onDelete}
        aria-label="Delete meeting"
        title="Delete meeting"
        className="shrink-0 px-3 text-faint opacity-0 transition-opacity hover:text-fg group-hover:opacity-100"
      >
        ✕
      </button>
    </li>
  );
}
