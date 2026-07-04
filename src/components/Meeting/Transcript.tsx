/**
 * Transcript — viewer/editor for transcript segments (PRD §3.3 #16, #17).
 * Hairline-ruled rows with a monospace timecode, an uppercase speaker label
 * (from diarization), and the text. Clicking a segment's text enables inline
 * editing; saving pushes via `api.updateSegment`.
 *
 * Speaker names: segments carry their RAW diarization label ("Speaker N").
 * `labels` maps raw → real display name (the label-once feature); clicking a
 * speaker chip renames it. The rename always targets the raw label, so a name
 * stays editable no matter how many times it is changed.
 */

import { useState } from "react";
import { api } from "@/lib/tauri";
import { formatTimestamp } from "@/lib/format";
import type { TranscriptSegment } from "@/lib/types";

export interface TranscriptProps {
  segments: TranscriptSegment[];
  /** Disable editing (e.g. while transcription is still running). */
  readOnly?: boolean;
  /** Raw diarization label → real display name. */
  labels?: Record<string, string>;
  /** Needed to persist a speaker rename. Renaming is disabled without it. */
  meetingId?: string;
  /** Called after a successful rename so the parent can refetch labels. */
  onRelabel?: () => void;
}

export function Transcript({
  segments,
  readOnly = false,
  labels = {},
  meetingId,
  onRelabel,
}: TranscriptProps) {
  if (segments.length === 0) {
    return <p className="text-sm text-muted">Transcript not available yet.</p>;
  }

  return (
    <ol>
      {segments.map((seg) => (
        <SegmentRow
          key={seg.id}
          segment={seg}
          readOnly={readOnly}
          labels={labels}
          meetingId={meetingId}
          onRelabel={onRelabel}
        />
      ))}
    </ol>
  );
}

function SegmentRow({
  segment,
  readOnly,
  labels,
  meetingId,
  onRelabel,
}: {
  segment: TranscriptSegment;
  readOnly: boolean;
  labels: Record<string, string>;
  meetingId?: string;
  onRelabel?: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(segment.text);
  const [editingSpeaker, setEditingSpeaker] = useState(false);

  const save = () => {
    setEditing(false);
    if (text !== segment.text) {
      void api.updateSegment(segment.id, text, segment.speaker).catch(() => {});
    }
  };

  const rawSpeaker = segment.speaker; // stable diarization label, never overwritten
  const displaySpeaker = rawSpeaker ? labels[rawSpeaker] ?? rawSpeaker : null;
  const canRename = !readOnly && !!meetingId && !!rawSpeaker;

  const saveSpeaker = (value: string) => {
    setEditingSpeaker(false);
    const name = value.trim();
    if (rawSpeaker && meetingId && name && name !== displaySpeaker) {
      void api
        .setSpeakerLabel(meetingId, rawSpeaker, name)
        .then(() => onRelabel?.())
        .catch(() => {});
    }
  };

  return (
    <li className="grid grid-cols-[3.25rem_1fr] gap-4 border-t border-line py-3 first:border-t-0">
      <span className="num pt-1 text-[11px] text-faint">
        {formatTimestamp(segment.start_time_ms)}
      </span>
      <div>
        {displaySpeaker &&
          (editingSpeaker ? (
            <input
              aria-label="Speaker name"
              autoFocus
              defaultValue={displaySpeaker}
              onBlur={(e) => saveSpeaker(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveSpeaker(e.currentTarget.value);
                if (e.key === "Escape") setEditingSpeaker(false);
              }}
              className="mr-2 w-28 border border-line-strong bg-bg px-1 py-0.5 text-xs text-fg focus:outline-none"
            />
          ) : canRename ? (
            <button
              type="button"
              aria-label={`Rename speaker ${displaySpeaker}`}
              onClick={() => setEditingSpeaker(true)}
              className="eyebrow mr-2 align-baseline text-fg hover:underline"
            >
              {displaySpeaker}
            </button>
          ) : (
            <span className="eyebrow mr-2 align-baseline text-fg">
              {displaySpeaker}
            </span>
          ))}
        {editing ? (
          <textarea
            autoFocus
            value={text}
            onChange={(e) => setText(e.target.value)}
            onBlur={save}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) save();
              if (e.key === "Escape") {
                setText(segment.text);
                setEditing(false);
              }
            }}
            className="mt-1 w-full resize-y border border-line-strong bg-bg p-2 text-sm text-fg focus:outline-none"
            rows={2}
          />
        ) : (
          <span
            className={`text-sm leading-relaxed text-fg ${
              readOnly ? "" : "cursor-text hover:bg-surface"
            }`}
            onClick={() => !readOnly && setEditing(true)}
          >
            {segment.text}
          </span>
        )}
      </div>
    </li>
  );
}
