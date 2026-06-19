/**
 * MeetingList — the meeting history view (PRD §3.5). Search box (FTS over
 * transcripts), status badges, tags, and a row click that opens the detail view.
 */

import { useState } from "react";
import { useMeetings } from "@/hooks/useMeetings";
import { useRecordingStore } from "@/stores/recordingStore";
import { Button } from "@/components/common/Button";
import { formatDateTime, formatDuration } from "@/lib/format";
import {
  MEETING_STATUS,
  MEETING_TYPE_LABELS,
  type MeetingStatus,
} from "@/lib/constants";
import type { Meeting } from "@/lib/types";

const STATUS_BADGE: Record<MeetingStatus, string> = {
  [MEETING_STATUS.Recording]:
    "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300",
  [MEETING_STATUS.Transcribing]:
    "bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300",
  [MEETING_STATUS.Completed]:
    "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300",
  [MEETING_STATUS.Error]:
    "bg-gray-200 text-gray-700 dark:bg-gray-700 dark:text-gray-300",
};

export function MeetingList() {
  const { meetings, loading, error, search, refresh } = useMeetings();
  const openMeeting = useRecordingStore((s) => s.openMeeting);
  const [query, setQuery] = useState("");

  return (
    <div className="mx-auto flex h-full w-full max-w-4xl flex-col gap-4 p-6">
      <div className="flex items-center gap-3">
        <input
          type="search"
          value={query}
          placeholder="Search transcripts…"
          aria-label="Search transcripts"
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void search(query);
          }}
          className="h-10 flex-1 rounded-md border border-gray-300 bg-white px-3 text-sm text-gray-900 placeholder-gray-400 focus:border-blue-500 focus:outline-none dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
        />
        <Button variant="secondary" onClick={() => void search(query)}>
          Search
        </Button>
        {query && (
          <Button
            variant="ghost"
            onClick={() => {
              setQuery("");
              void refresh();
            }}
          >
            Clear
          </Button>
        )}
      </div>

      {loading && (
        <p className="text-sm text-gray-500 dark:text-gray-400">Loading…</p>
      )}
      {error && (
        <p className="text-sm text-recording" role="alert">
          {error}
        </p>
      )}
      {!loading && meetings.length === 0 && (
        <p className="text-sm text-gray-500 dark:text-gray-400">
          No meetings yet. Press ● Record to start.
        </p>
      )}

      <ul className="flex flex-col gap-2">
        {meetings.map((m) => (
          <MeetingRow key={m.id} meeting={m} onOpen={() => openMeeting(m.id)} />
        ))}
      </ul>
    </div>
  );
}

function MeetingRow({
  meeting,
  onOpen,
}: {
  meeting: Meeting;
  onOpen: () => void;
}) {
  return (
    <li>
      <button
        type="button"
        onClick={onOpen}
        className="flex w-full items-center justify-between gap-4 rounded-lg border border-gray-200 bg-white p-4 text-left transition-colors hover:border-blue-400 hover:bg-blue-50/40 dark:border-gray-800 dark:bg-gray-900 dark:hover:border-blue-600 dark:hover:bg-gray-800"
      >
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="truncate font-medium text-gray-900 dark:text-gray-100">
              {meeting.title ?? "Untitled meeting"}
            </span>
            {meeting.meeting_type && (
              <span className="rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-600 dark:bg-gray-800 dark:text-gray-400">
                {MEETING_TYPE_LABELS[meeting.meeting_type]}
              </span>
            )}
          </div>
          <p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400">
            {formatDateTime(meeting.start_time)} ·{" "}
            {formatDuration(meeting.duration_seconds)}
          </p>
          {meeting.tags.length > 0 && (
            <div className="mt-1 flex flex-wrap gap-1">
              {meeting.tags.map((t) => (
                <span
                  key={t}
                  className="rounded-full bg-blue-50 px-2 py-0.5 text-xs text-blue-700 dark:bg-blue-900/30 dark:text-blue-300"
                >
                  #{t}
                </span>
              ))}
            </div>
          )}
        </div>
        <span
          className={`shrink-0 rounded-full px-2 py-0.5 text-xs font-medium ${STATUS_BADGE[meeting.status]}`}
        >
          {meeting.status}
        </span>
      </button>
    </li>
  );
}
