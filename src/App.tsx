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
    <div className="flex h-full flex-col bg-white text-gray-900 dark:bg-gray-950 dark:text-gray-100">
      <header className="flex items-center justify-between gap-4 border-b border-gray-200 px-6 py-3 dark:border-gray-800">
        <button
          type="button"
          onClick={() => useRecordingStore.getState().navigate(VIEWS.Meetings)}
          className="flex items-center gap-3 text-left"
        >
          <div>
            <h1 className="text-base font-semibold">MeetingRecordApp</h1>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              v1.0 — Windows, audio-only, local-first
            </p>
          </div>
        </button>

        <div className="flex items-center gap-4">
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
