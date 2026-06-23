/**
 * TrayMenu — the quick-control surface that maps to the native tray context menu
 * (Rust `tray.rs` builds the real OS menu; this is the in-window equivalent used
 * in the header). Start/stop/pause + jump to history/settings.
 */

import { useRecording } from "@/hooks/useRecording";
import { useRecordingStore } from "@/stores/recordingStore";
import { Button } from "@/components/common/Button";
import { Tooltip } from "@/components/common/Tooltip";
import { SHORTCUTS, VIEWS } from "@/lib/constants";

export function TrayMenu() {
  const rec = useRecording();
  const navigate = useRecordingStore((s) => s.navigate);

  return (
    <div className="flex items-center gap-2">
      {!rec.isActive ? (
        <Tooltip
          label={`Start recording (${SHORTCUTS.toggleRecording})`}
          side="bottom"
        >
          <Button variant="danger" size="sm" onClick={() => rec.start()}>
            ● Record
          </Button>
        </Tooltip>
      ) : (
        <>
          {rec.isRecording ? (
            <Button variant="secondary" size="sm" onClick={() => rec.pause()}>
              Pause
            </Button>
          ) : (
            <Button variant="secondary" size="sm" onClick={() => rec.resume()}>
              Resume
            </Button>
          )}
          <Button variant="danger" size="sm" onClick={() => rec.stop()}>
            Stop
          </Button>
        </>
      )}

      <span className="mx-1 h-5 w-px bg-line" aria-hidden />

      <Button
        variant="ghost"
        size="sm"
        onClick={() => navigate(VIEWS.Meetings)}
      >
        History
      </Button>
      <Button
        variant="ghost"
        size="sm"
        onClick={() => navigate(VIEWS.Settings)}
      >
        Settings
      </Button>
    </div>
  );
}
