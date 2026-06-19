/**
 * Settings state (Zustand). Mirrors the `settings` table; on the backend each
 * field is persisted as a `settings` row keyed by `SETTINGS_KEYS`. Here we keep
 * a typed object plus a `setField` action that also pushes to the backend (best
 * effort — failures are swallowed in mock/browser mode).
 *
 * The theme field drives the `dark` class on <html> via `applyTheme`.
 */

import { create } from "zustand";
import {
  AI_PROVIDERS,
  DEFAULT_AI_PROVIDER,
  DEFAULT_OLLAMA_ENDPOINT,
  DEFAULT_THEME,
  DEFAULT_WHISPER_MODEL,
  PROVIDER_META,
  SETTINGS_KEYS,
  type AiProvider,
  type Theme,
  type TranscriptionLanguage,
  type WhisperModel,
} from "@/lib/constants";
import { api } from "@/lib/tauri";

export interface SettingsState {
  // general
  theme: Theme;
  autoStart: boolean;
  minimizeToTray: boolean;
  autoDetectMeetings: boolean;
  storageDir: string;
  // audio
  micDeviceId: string | null;
  systemAudioEnabled: boolean;
  keepDualTrack: boolean;
  // transcription
  whisperModel: WhisperModel;
  language: TranscriptionLanguage;
  diarizationEnabled: boolean;
  // ai
  aiProvider: AiProvider;
  aiModel: string;
  ollamaEndpoint: string;
}

const DEFAULTS: SettingsState = {
  theme: DEFAULT_THEME,
  autoStart: false,
  minimizeToTray: true,
  autoDetectMeetings: true,
  storageDir: "",
  micDeviceId: null,
  systemAudioEnabled: true,
  keepDualTrack: false,
  whisperModel: DEFAULT_WHISPER_MODEL,
  language: "auto",
  diarizationEnabled: true,
  aiProvider: DEFAULT_AI_PROVIDER,
  aiModel: PROVIDER_META[DEFAULT_AI_PROVIDER].models[0],
  ollamaEndpoint: DEFAULT_OLLAMA_ENDPOINT,
};

/** Map each settings field to its backend `settings.key`. */
const FIELD_KEY: Partial<Record<keyof SettingsState, string>> = {
  theme: SETTINGS_KEYS.Theme,
  autoStart: SETTINGS_KEYS.AutoStart,
  minimizeToTray: SETTINGS_KEYS.MinimizeToTray,
  autoDetectMeetings: SETTINGS_KEYS.AutoDetectMeetings,
  storageDir: SETTINGS_KEYS.StorageDir,
  micDeviceId: SETTINGS_KEYS.MicDeviceId,
  systemAudioEnabled: SETTINGS_KEYS.SystemAudioEnabled,
  keepDualTrack: SETTINGS_KEYS.KeepDualTrack,
  whisperModel: SETTINGS_KEYS.WhisperModel,
  language: SETTINGS_KEYS.Language,
  diarizationEnabled: SETTINGS_KEYS.DiarizationEnabled,
  aiProvider: SETTINGS_KEYS.AiProvider,
  aiModel: SETTINGS_KEYS.AiModel,
  ollamaEndpoint: SETTINGS_KEYS.OllamaEndpoint,
};

interface SettingsActions {
  /** Update one field locally + persist to backend (best effort). */
  setField: <K extends keyof SettingsState>(
    key: K,
    value: SettingsState[K],
  ) => void;
  /** Switch provider and reset the model to that provider's first option. */
  setProvider: (provider: AiProvider) => void;
  /** Load persisted settings from the backend (no-op-safe in mock mode). */
  load: () => Promise<void>;
  reset: () => void;
}

export type SettingsStore = SettingsState & SettingsActions;

export const useSettingsStore = create<SettingsStore>((set, get) => ({
  ...DEFAULTS,

  setField: (key, value) => {
    set({ [key]: value } as Pick<SettingsState, typeof key>);
    if (key === "theme") applyTheme(value as Theme);
    const backendKey = FIELD_KEY[key];
    if (backendKey) {
      void api.setSetting(backendKey, serialize(value)).catch(() => {
        /* ignore in mock/browser mode */
      });
    }
  },

  setProvider: (provider) => {
    const model = PROVIDER_META[provider].models[0];
    get().setField("aiProvider", provider);
    get().setField("aiModel", model);
  },

  load: async () => {
    try {
      const rows = await api.getSettings();
      if (rows && rows[SETTINGS_KEYS.Theme]) {
        const t = rows[SETTINGS_KEYS.Theme] as Theme;
        set({ theme: t });
        applyTheme(t);
      }
      // Backend hydration of the remaining typed fields is wired in Integrate;
      // mock mode returns {} so we keep the defaults.
    } catch {
      /* ignore */
    }
  },

  reset: () => set({ ...DEFAULTS }),
}));

function serialize(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value);
}

/**
 * Apply the chosen theme to <html>. "system" follows the OS preference. Exposed
 * so `main.tsx` can call it once on boot (before React renders).
 */
export function applyTheme(theme: Theme): void {
  if (typeof document === "undefined") return;
  const prefersDark =
    typeof window !== "undefined" &&
    window.matchMedia?.("(prefers-color-scheme: dark)").matches;
  const dark = theme === "dark" || (theme === "system" && !!prefersDark);
  document.documentElement.classList.toggle("dark", dark);
}

/** Convenience selector: is the active provider a cloud (API-key) provider? */
export function isCloudProvider(provider: AiProvider): boolean {
  return provider !== AI_PROVIDERS.Ollama;
}
