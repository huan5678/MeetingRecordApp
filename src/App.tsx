/**
 * App shell — the main window. Header reflects the tray (status + quick
 * controls); body routes between the meeting history, a meeting's detail, and
 * settings; the floating mini-panel overlays while recording.
 *
 * Navigation is a tiny store-driven switch (no router dependency — the app is a
 * single window with three views). Global record/stop shortcuts are bound here.
 */

import { useEffect } from "react";
import { TrayIcon } from "@/components/Tray/TrayIcon";
import { TrayMenu } from "@/components/Tray/TrayMenu";
import { MiniPanel } from "@/components/Floating/MiniPanel";
import { MeetingList } from "@/components/Meeting/MeetingList";
import { MeetingDetail } from "@/components/Meeting/MeetingDetail";
import { SettingsPage } from "@/components/Settings/SettingsPage";
import { useRecordingStore } from "@/stores/recordingStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useRecording } from "@/hooks/useRecording";
import { VIEWS } from "@/lib/constants";

export default function App() {
  const view = useRecordingStore((s) => s.view);
  const loadSettings = useSettingsStore((s) => s.load);
  const rec = useRecording();

  // Hydrate persisted settings once on boot.
  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  // Global keyboard shortcuts (PRD §3.7 #41).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey && e.shiftKey;
      if (mod && (e.key === "R" || e.key === "r")) {
        e.preventDefault();
        void rec.toggle();
      } else if (mod && (e.key === "S" || e.key === "s")) {
        e.preventDefault();
        if (rec.isActive) void rec.stop();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [rec]);

  return (
    <div className="flex h-full flex-col bg-bg text-fg">
      <header className="flex items-center justify-between gap-4 border-b border-line bg-bg px-6 py-3.5">
        <button
          type="button"
          onClick={() => useRecordingStore.getState().navigate(VIEWS.Meetings)}
          className="group flex flex-col items-start gap-1 text-left"
        >
          <span className="font-display text-[15px] font-semibold leading-none tracking-tight text-fg">
            Meeting
            <span className="text-muted transition-colors group-hover:text-fg">
              Record
            </span>
          </span>
          <span className="eyebrow">Local-first recorder</span>
        </button>

        <div className="flex items-center gap-5">
          <TrayIcon />
          <TrayMenu />
        </div>
      </header>

      <main className="flex-1 overflow-hidden">
        {view === VIEWS.Meetings && <MeetingList />}
        {view === VIEWS.Detail && <MeetingDetail />}
        {view === VIEWS.Settings && <SettingsPage />}
      </main>

      <MiniPanel />
    </div>
  );
}
