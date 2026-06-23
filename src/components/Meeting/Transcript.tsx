/**
 * Transcript — viewer/editor for transcript segments (PRD §3.3 #16, #17).
 * Hairline-ruled rows with a monospace timecode, an uppercase speaker label
 * (from diarization), and the text. Clicking a segment's text enables inline
 * editing; saving pushes via `api.updateSegment`.
 */

import { useState } from "react";
import { api } from "@/lib/tauri";
import { formatTimestamp } from "@/lib/format";
import type { TranscriptSegment } from "@/lib/types";

export interface TranscriptProps {
  segments: TranscriptSegment[];
  /** Disable editing (e.g. while transcription is still running). */
  readOnly?: boolean;
}

export function Transcript({ segments, readOnly = false }: TranscriptProps) {
  if (segments.length === 0) {
    return <p className="text-sm text-muted">Transcript not available yet.</p>;
  }

  return (
    <ol>
      {segments.map((seg) => (
        <SegmentRow key={seg.id} segment={seg} readOnly={readOnly} />
      ))}
    </ol>
  );
}

function SegmentRow({
  segment,
  readOnly,
}: {
  segment: TranscriptSegment;
  readOnly: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(segment.text);

  const save = () => {
    setEditing(false);
    if (text !== segment.text) {
      void api.updateSegment(segment.id, text, segment.speaker).catch(() => {});
    }
  };

  return (
    <li className="grid grid-cols-[3.25rem_1fr] gap-4 border-t border-line py-3 first:border-t-0">
      <span className="num pt-1 text-[11px] text-faint">
        {formatTimestamp(segment.start_time_ms)}
      </span>
      <div>
        {segment.speaker && (
          <span className="eyebrow mr-2 align-baseline text-fg">
            {segment.speaker}
          </span>
        )}
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
