/**
 * SettingsPage — tabbed settings container (Audio / AI / General), per
 * docs/STRUCTURE.md. Tab state is local; each panel reads/writes `settingsStore`.
 */

import { useState } from "react";
import { AudioSettings } from "@/components/Settings/AudioSettings";
import { AISettings } from "@/components/Settings/AISettings";
import { GeneralSettings } from "@/components/Settings/GeneralSettings";

type Tab = "audio" | "ai" | "general";

const TABS: { id: Tab; label: string }[] = [
  { id: "audio", label: "Audio" },
  { id: "ai", label: "AI & Transcription" },
  { id: "general", label: "General" },
];

export function SettingsPage() {
  const [tab, setTab] = useState<Tab>("audio");

  return (
    <div className="mx-auto flex h-full w-full max-w-3xl flex-col gap-6 p-6">
      <h1 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
        Settings
      </h1>

      <nav className="flex gap-1 border-b border-gray-200 dark:border-gray-800">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className={`-mb-px border-b-2 px-3 py-2 text-sm font-medium transition-colors ${
              tab === t.id
                ? "border-blue-600 text-blue-600 dark:text-blue-400"
                : "border-transparent text-gray-500 hover:text-gray-800 dark:text-gray-400 dark:hover:text-gray-200"
            }`}
          >
            {t.label}
          </button>
        ))}
      </nav>

      <div className="flex-1 overflow-auto pb-6">
        {tab === "audio" && <AudioSettings />}
        {tab === "ai" && <AISettings />}
        {tab === "general" && <GeneralSettings />}
      </div>
    </div>
  );
}
