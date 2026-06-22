/**
 * AISettings — AI summarization provider/model + API keys (PRD §3.8 #45/#46) and
 * whisper transcription model (#44). Ollama is the local default (no key); cloud
 * providers (OpenAI/Claude/Gemini) reveal an API-key field stored in the OS
 * keychain via the backend (`set_api_key`).
 */

import { useEffect, useState } from "react";
import { useSettingsStore } from "@/stores/settingsStore";
import { api } from "@/lib/tauri";
import { Button } from "@/components/common/Button";
import { Field, Row, Toggle } from "@/components/Settings/controls";
import {
  AI_PROVIDERS,
  DEFAULT_OLLAMA_ENDPOINT,
  GEMINI_TRANSCRIBE_MODELS,
  PROVIDER_META,
  TRANSCRIPTION_ENGINES,
  TRANSCRIPTION_LANGUAGES,
  WHISPER_MODELS,
  type AiProvider,
  type TranscriptionEngine,
  type TranscriptionLanguage,
  type WhisperModel,
} from "@/lib/constants";

export function AISettings() {
  const provider = useSettingsStore((s) => s.aiProvider);
  const model = useSettingsStore((s) => s.aiModel);
  const ollamaEndpoint = useSettingsStore((s) => s.ollamaEndpoint);
  const whisperModel = useSettingsStore((s) => s.whisperModel);
  const language = useSettingsStore((s) => s.language);
  const diarizationEnabled = useSettingsStore((s) => s.diarizationEnabled);
  const transcriptionEngine = useSettingsStore((s) => s.transcriptionEngine);
  const geminiModel = useSettingsStore((s) => s.geminiModel);
  const setField = useSettingsStore((s) => s.setField);
  const setProvider = useSettingsStore((s) => s.setProvider);

  const isCloud = provider !== AI_PROVIDERS.Ollama;
  // Gemini engine is in play for "gemini" and "auto" (when a key is set).
  const geminiEngine = transcriptionEngine !== "whisper";

  return (
    <div className="space-y-8">
      <section className="space-y-5">
        <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
          Summarization
        </h3>

        <Field
          label="Provider"
          hint="Ollama runs locally (default, private). Cloud providers are optional and send the transcript out."
        >
          <select
            value={provider}
            onChange={(e) => setProvider(e.target.value as AiProvider)}
            className="block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          >
            {Object.values(PROVIDER_META).map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
        </Field>

        <Field label="Model">
          <select
            value={model}
            onChange={(e) => setField("aiModel", e.target.value)}
            className="block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          >
            {PROVIDER_META[provider].models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </Field>

        {!isCloud && (
          <Field label="Ollama endpoint">
            <input
              type="text"
              value={ollamaEndpoint}
              placeholder={DEFAULT_OLLAMA_ENDPOINT}
              onChange={(e) => setField("ollamaEndpoint", e.target.value)}
              className="block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
            />
          </Field>
        )}

        {isCloud && <ApiKeyField provider={provider} />}
      </section>

      <section className="space-y-5">
        <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
          Transcription
        </h3>

        <Field
          label="轉錄引擎"
          hint="Gemini:錄音上傳雲端,一次產生繁中逐字稿(含語者+時間戳)+ 摘要,免本地模型;需 Gemini API key。"
        >
          <select
            value={transcriptionEngine}
            onChange={(e) =>
              setField(
                "transcriptionEngine",
                e.target.value as TranscriptionEngine,
              )
            }
            className="block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          >
            {TRANSCRIPTION_ENGINES.map((en) => (
              <option key={en.id} value={en.id}>
                {en.label}
              </option>
            ))}
          </select>
        </Field>

        {geminiEngine && (
          <>
            <Field label="Gemini 模型" hint="多模態音訊模型;Flash 系列速度/成本最佳。">
              <select
                value={geminiModel}
                onChange={(e) => setField("geminiModel", e.target.value)}
                className="block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
              >
                {GEMINI_TRANSCRIBE_MODELS.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label}
                  </option>
                ))}
              </select>
            </Field>
            <p className="rounded-md bg-amber-50 p-2 text-xs text-amber-800 dark:bg-amber-900/30 dark:text-amber-300">
              ⚠️ Gemini 會把整段錄音上傳到 Google 雲端。請在上方 Summarization 把
              Provider 設為 Gemini 並填入 API key(轉錄與摘要共用同一把 key)。
            </p>
          </>
        )}

        <Field
          label="Whisper model"
          hint="本地引擎(或 Gemini 失敗時的備援)用的模型。"
        >
          <select
            value={whisperModel}
            onChange={(e) =>
              setField("whisperModel", e.target.value as WhisperModel)
            }
            className="block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          >
            {WHISPER_MODELS.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
        </Field>

        <Field label="Language">
          <select
            value={language}
            onChange={(e) =>
              setField("language", e.target.value as TranscriptionLanguage)
            }
            className="block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          >
            {TRANSCRIPTION_LANGUAGES.map((l) => (
              <option key={l.id} value={l.id}>
                {l.label}
              </option>
            ))}
          </select>
        </Field>

        <Row
          label="Speaker diarization"
          hint="Basic who-said-what via sherpa-onnx. Adds a model download."
        >
          <Toggle
            checked={diarizationEnabled}
            onChange={(v) => setField("diarizationEnabled", v)}
          />
        </Row>
      </section>
    </div>
  );
}

/** API-key input for a cloud provider. Keys are stored in the OS keychain. */
function ApiKeyField({ provider }: { provider: AiProvider }) {
  const [hasKey, setHasKey] = useState(false);
  const [value, setValue] = useState("");
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    setValue("");
    setSaved(false);
    void api
      .hasApiKey(provider)
      .then(setHasKey)
      .catch(() => setHasKey(false));
  }, [provider]);

  const save = async () => {
    await api.setApiKey(provider, value).catch(() => {});
    setHasKey(true);
    setSaved(true);
    setValue("");
  };

  return (
    <Field
      label={`${PROVIDER_META[provider].label} API key`}
      hint="Stored securely in the OS keychain — never in plain text."
    >
      <div className="flex gap-2">
        <input
          type="password"
          value={value}
          placeholder={hasKey ? "•••••••• (saved)" : "Paste API key"}
          onChange={(e) => {
            setValue(e.target.value);
            setSaved(false);
          }}
          className="block flex-1 rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
        />
        <Button
          variant="secondary"
          onClick={() => void save()}
          disabled={!value}
        >
          {saved ? "Saved" : "Save"}
        </Button>
      </div>
    </Field>
  );
}
