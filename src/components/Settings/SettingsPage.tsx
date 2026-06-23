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
    <div className="mx-auto flex h-full w-full max-w-2xl flex-col gap-6 px-8 pt-8">
      <div>
        <span className="eyebrow">Preferences</span>
        <h1 className="mt-2 font-display text-2xl font-medium tracking-tight text-fg">
          Settings
        </h1>
      </div>

      <nav className="flex gap-6 border-b border-line">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className={`eyebrow -mb-px border-b-2 py-3 transition-colors ${
              tab === t.id
                ? "border-fg text-fg"
                : "border-transparent text-faint hover:text-fg"
            }`}
          >
            {t.label}
          </button>
        ))}
      </nav>

      <div className="flex-1 overflow-auto pb-8">
        {tab === "audio" && <AudioSettings />}
        {tab === "ai" && <AISettings />}
        {tab === "general" && <GeneralSettings />}
      </div>
    </div>
  );
}
