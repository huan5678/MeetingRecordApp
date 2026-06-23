/**
 * MiniPanel — the floating recording mini-panel (PRD §3.7 #38). Shows the live
 * timer, mic / system audio meters, and pause/resume + stop controls so the user
 * can control recording without leaving their meeting.
 *
 * In the real app this renders in a separate always-on-top Tauri window; in this
 * skeleton it's a fixed overlay shown while recording is active.
 */

import { useRecording } from "@/hooks/useRecording";
import { AudioLevel } from "@/components/Floating/AudioLevel";
import { Button } from "@/components/common/Button";
import { Tooltip } from "@/components/common/Tooltip";
import { formatClock } from "@/lib/format";
import { SHORTCUTS } from "@/lib/constants";

export function MiniPanel() {
  const rec = useRecording();
  if (!rec.isActive) return null;

  return (
    <div className="fixed bottom-4 right-4 z-40 w-72 border border-line-strong bg-bg/95 p-4 shadow-2xl backdrop-blur">
      <div className="mb-3 flex items-baseline justify-between">
        <div className="flex items-center gap-2">
          <span
            className={`h-1.5 w-1.5 rounded-full ${
              rec.isRecording ? "animate-pulse bg-fg" : "border border-fg"
            }`}
            aria-hidden
          />
          <span className="eyebrow">{rec.isRecording ? "Recording" : "Paused"}</span>
        </div>
        <span className="num text-base text-fg">
          {formatClock(rec.elapsedSeconds)}
        </span>
      </div>

      <div className="mb-3 space-y-1.5">
        <AudioLevel label="Mic" level={rec.micLevel} />
        <AudioLevel label="Sys" level={rec.systemLevel} />
      </div>

      <div className="flex gap-2">
        {rec.isRecording ? (
          <Tooltip label="Pause">
            <Button variant="secondary" size="sm" onClick={() => rec.pause()}>
              Pause
            </Button>
          </Tooltip>
        ) : (
          <Tooltip label="Resume">
            <Button variant="secondary" size="sm" onClick={() => rec.resume()}>
              Resume
            </Button>
          </Tooltip>
        )}
        <Tooltip label={`Stop (${SHORTCUTS.stopRecording})`}>
          <Button
            variant="danger"
            size="sm"
            className="flex-1"
            onClick={() => rec.stop()}
          >
            Stop
          </Button>
        </Tooltip>
      </div>
    </div>
  );
}
