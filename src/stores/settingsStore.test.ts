import { describe, it, expect, beforeEach } from "vitest";
import {
  useSettingsStore,
  applyTheme,
  isCloudProvider,
} from "@/stores/settingsStore";
import { AI_PROVIDERS, PROVIDER_META } from "@/lib/constants";

describe("settingsStore", () => {
  beforeEach(() => {
    useSettingsStore.getState().reset();
    document.documentElement.classList.remove("dark");
  });

  it("defaults to the Ollama (local) provider", () => {
    expect(useSettingsStore.getState().aiProvider).toBe(AI_PROVIDERS.Ollama);
  });

  it("setProvider switches provider and resets the model", () => {
    useSettingsStore.getState().setProvider(AI_PROVIDERS.OpenAi);
    const s = useSettingsStore.getState();
    expect(s.aiProvider).toBe(AI_PROVIDERS.OpenAi);
    expect(s.aiModel).toBe(PROVIDER_META[AI_PROVIDERS.OpenAi].models[0]);
  });

  it("setField updates a single field", () => {
    useSettingsStore.getState().setField("diarizationEnabled", false);
    expect(useSettingsStore.getState().diarizationEnabled).toBe(false);
  });

  it("setting theme to dark toggles the html dark class", () => {
    useSettingsStore.getState().setField("theme", "dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    useSettingsStore.getState().setField("theme", "light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("applyTheme('light') removes the dark class regardless of OS", () => {
    document.documentElement.classList.add("dark");
    applyTheme("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("isCloudProvider classifies providers", () => {
    expect(isCloudProvider(AI_PROVIDERS.Ollama)).toBe(false);
    expect(isCloudProvider(AI_PROVIDERS.OpenAi)).toBe(true);
    expect(isCloudProvider(AI_PROVIDERS.Claude)).toBe(true);
    expect(isCloudProvider(AI_PROVIDERS.Gemini)).toBe(true);
  });
});
