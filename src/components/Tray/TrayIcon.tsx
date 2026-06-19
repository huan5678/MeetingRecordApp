/**
 * TrayIcon — a small status dot + elapsed timer shown in the app header to
 * mirror what the OS system-tray icon conveys (PRD §3.7 #37). The native tray
 * itself is owned by Rust (`tray.rs`); this is the in-window reflection of it.
 */

import { useRecording } from "@/hooks/useRecording";
import { formatClock } from "@/lib/format";

export function TrayIcon() {
  const rec = useRecording();

  const dot = rec.isRecording
    ? "animate-pulse bg-recording"
    : rec.isPaused
      ? "bg-amber-500"
      : "bg-gray-400 dark:bg-gray-600";

  return (
    <div
      className="flex items-center gap-2"
      title={rec.isActive ? "Recording in progress" : "Idle"}
    >
      <span className={`h-2.5 w-2.5 rounded-full ${dot}`} aria-hidden />
      <span className="text-xs text-gray-600 dark:text-gray-400">
        {rec.isActive ? (
          <span className="font-mono tabular-nums">
            {formatClock(rec.elapsedSeconds)}
          </span>
        ) : (
          "Idle"
        )}
      </span>
    </div>
  );
}
