/**
 * Transcript — viewer/editor for transcript segments (PRD §3.3 #16, #17).
 * Shows per-segment timestamp, speaker label (from diarization), and text.
 * Clicking a segment's text enables inline editing; saving pushes via
 * `api.updateSegment`. Speakers get a stable colour for quick scanning.
 */

import { useMemo, useState } from "react";
import { api } from "@/lib/tauri";
import { formatTimestamp } from "@/lib/format";
import type { TranscriptSegment } from "@/lib/types";

export interface TranscriptProps {
  segments: TranscriptSegment[];
  /** Disable editing (e.g. while transcription is still running). */
  readOnly?: boolean;
}

const SPEAKER_COLORS = [
  "text-blue-600 dark:text-blue-400",
  "text-emerald-600 dark:text-emerald-400",
  "text-purple-600 dark:text-purple-400",
  "text-orange-600 dark:text-orange-400",
  "text-pink-600 dark:text-pink-400",
];

/** Deterministic colour per distinct speaker label. */
function useSpeakerColors(segments: TranscriptSegment[]): Map<string, string> {
  return useMemo(() => {
    const map = new Map<string, string>();
    let i = 0;
    for (const s of segments) {
      const sp = s.speaker ?? "";
      if (sp && !map.has(sp)) {
        map.set(sp, SPEAKER_COLORS[i % SPEAKER_COLORS.length]);
        i += 1;
      }
    }
    return map;
  }, [segments]);
}

export function Transcript({ segments, readOnly = false }: TranscriptProps) {
  const colors = useSpeakerColors(segments);

  if (segments.length === 0) {
    return (
      <p className="text-sm text-gray-500 dark:text-gray-400">
        Transcript not available yet.
      </p>
    );
  }

  return (
    <ol className="flex flex-col gap-3">
      {segments.map((seg) => (
        <SegmentRow
          key={seg.id}
          segment={seg}
          color={seg.speaker ? colors.get(seg.speaker) : undefined}
          readOnly={readOnly}
        />
      ))}
    </ol>
  );
}

function SegmentRow({
  segment,
  color,
  readOnly,
}: {
  segment: TranscriptSegment;
  color?: string;
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
    <li className="grid grid-cols-[auto_1fr] gap-3">
      <span className="pt-0.5 font-mono text-xs tabular-nums text-gray-400 dark:text-gray-500">
        {formatTimestamp(segment.start_time_ms)}
      </span>
      <div>
        {segment.speaker && (
          <span className={`mr-2 text-xs font-semibold ${color ?? ""}`}>
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
            className="mt-1 w-full resize-y rounded border border-blue-400 bg-white p-2 text-sm text-gray-900 focus:outline-none dark:bg-gray-800 dark:text-gray-100"
            rows={2}
          />
        ) : (
          <span
            className={`text-sm text-gray-800 dark:text-gray-200 ${
              readOnly ? "" : "cursor-text hover:bg-yellow-50 dark:hover:bg-gray-800"
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
