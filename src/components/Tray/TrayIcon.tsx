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
    ? "bg-fg animate-pulse"
    : rec.isPaused
      ? "border border-fg"
      : "bg-faint";

  return (
    <div
      className="flex items-center gap-2.5"
      title={rec.isActive ? "Recording in progress" : "Idle"}
    >
      <span className={`h-2 w-2 rounded-full ${dot}`} aria-hidden />
      {rec.isActive ? (
        <span className="num text-[13px] text-fg">
          {formatClock(rec.elapsedSeconds)}
        </span>
      ) : (
        <span className="eyebrow">Idle</span>
      )}
    </div>
  );
}
