/**
 * GeneralSettings — appearance + startup + storage (PRD §3.7 #40, §3.8 #47/#48).
 * Theme (system/light/dark), auto-start, minimize-to-tray, meeting auto-detect,
 * storage location, and a storage-usage readout.
 */

import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useSettingsStore } from "@/stores/settingsStore";
import { api, isTauri, type StorageUsageDto } from "@/lib/tauri";
import { Field, Row, Toggle } from "@/components/Settings/controls";
import { formatBytes } from "@/lib/format";
import { THEMES, type Theme, SHORTCUTS } from "@/lib/constants";

export function GeneralSettings() {
  const theme = useSettingsStore((s) => s.theme);
  const autoStart = useSettingsStore((s) => s.autoStart);
  const minimizeToTray = useSettingsStore((s) => s.minimizeToTray);
  const autoDetectMeetings = useSettingsStore((s) => s.autoDetectMeetings);
  const storageDir = useSettingsStore((s) => s.storageDir);
  const setField = useSettingsStore((s) => s.setField);

  const [usage, setUsage] = useState<StorageUsageDto | null>(null);
  useEffect(() => {
    void api
      .getStorageUsage()
      .then(setUsage)
      .catch(() => setUsage(null));
  }, []);

  return (
    <div className="space-y-5">
      <Field label="Theme" hint="“System” follows your OS appearance.">
        <select
          value={theme}
          onChange={(e) => setField("theme", e.target.value as Theme)}
          className="block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
        >
          {THEMES.map((t) => (
            <option key={t} value={t}>
              {t[0].toUpperCase() + t.slice(1)}
            </option>
          ))}
        </select>
      </Field>

      <Row label="Start with system" hint="Launch on login.">
        <Toggle checked={autoStart} onChange={(v) => setField("autoStart", v)} />
      </Row>

      <Row label="Minimize to tray" hint="Keep running in the background.">
        <Toggle
          checked={minimizeToTray}
          onChange={(v) => setField("minimizeToTray", v)}
        />
      </Row>

      <Row
        label="Auto-detect meetings"
        hint="Prompt to record when Teams / Google Meet is focused."
      >
        <Toggle
          checked={autoDetectMeetings}
          onChange={(v) => setField("autoDetectMeetings", v)}
        />
      </Row>

      <Field
        label="Storage location"
        hint="New recordings are saved here. Existing recordings stay where they were saved."
      >
        <div className="flex gap-2">
          <input
            type="text"
            value={storageDir}
            placeholder="Default app data folder"
            onChange={(e) => setField("storageDir", e.target.value)}
            className="block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          />
          {isTauri() && (
            <button
              type="button"
              onClick={async () => {
                const dir = await open({
                  directory: true,
                  defaultPath: storageDir || undefined,
                });
                if (typeof dir === "string") setField("storageDir", dir);
              }}
              className="shrink-0 rounded-md border border-gray-300 bg-white px-3 text-sm font-medium hover:bg-gray-50 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100 dark:hover:bg-gray-700"
            >
              Browse…
            </button>
          )}
        </div>
      </Field>

      <Row label="Storage used">
        <span className="text-sm text-gray-700 dark:text-gray-300">
          {usage
            ? `${formatBytes(usage.totalBytes)} · ${usage.meetingCount} meetings`
            : "—"}
        </span>
      </Row>

      <Field label="Keyboard shortcuts">
        <ul className="space-y-1 text-sm text-gray-700 dark:text-gray-300">
          <li className="flex justify-between">
            <span>Start / stop recording</span>
            <kbd className="rounded bg-gray-100 px-1.5 py-0.5 text-xs dark:bg-gray-800">
              {SHORTCUTS.toggleRecording}
            </kbd>
          </li>
          <li className="flex justify-between">
            <span>Stop recording</span>
            <kbd className="rounded bg-gray-100 px-1.5 py-0.5 text-xs dark:bg-gray-800">
              {SHORTCUTS.stopRecording}
            </kbd>
          </li>
        </ul>
      </Field>
    </div>
  );
}
