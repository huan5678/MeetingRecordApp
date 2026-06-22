/**
 * MeetingPlayer — audio playback surface for a meeting's recording (PRD §3.5
 * #28). v1.0 is audio-only, so this is a simple <audio> element fed the media
 * file path. In Tauri the path is converted to an asset URL; in the browser
 * skeleton there's no file, so we show a placeholder.
 */

import { convertFileSrc } from "@tauri-apps/api/core";
import { formatBytes, formatDuration } from "@/lib/format";
import { isTauri } from "@/lib/tauri";
import type { MediaFile } from "@/lib/types";

export interface MeetingPlayerProps {
  media: MediaFile[];
}

export function MeetingPlayer({ media }: MeetingPlayerProps) {
  const audio = media.find((m) => m.file_type === "audio");

  if (!audio) {
    return (
      <div className="rounded-lg border border-dashed border-gray-300 p-4 text-sm text-gray-500 dark:border-gray-700 dark:text-gray-400">
        No audio file for this meeting.
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4 dark:border-gray-800 dark:bg-gray-900">
      <div className="mb-2 flex items-center justify-between text-xs text-gray-500 dark:text-gray-400">
        <span className="truncate" title={audio.file_path}>
          {audio.format?.toUpperCase() ?? "AUDIO"} ·{" "}
          {formatDuration(audio.duration_seconds)}
        </span>
        <span>{formatBytes(audio.file_size_bytes)}</span>
      </div>
      {isTauri() ? (
        <audio controls className="w-full" src={convertFileSrc(audio.file_path)}>
          Your browser does not support audio playback.
        </audio>
      ) : (
        <div className="rounded bg-gray-100 px-3 py-6 text-center text-sm text-gray-500 dark:bg-gray-800 dark:text-gray-400">
          Audio playback available in the desktop app.
        </div>
      )}
    </div>
  );
}
