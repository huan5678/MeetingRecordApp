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
  PROVIDER_META,
  TRANSCRIPTION_LANGUAGES,
  WHISPER_MODELS,
  type AiProvider,
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
  const setField = useSettingsStore((s) => s.setField);
  const setProvider = useSettingsStore((s) => s.setProvider);

  const isCloud = provider !== AI_PROVIDERS.Ollama;

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
          label="Whisper model"
          hint="Default base/small. Larger models are more accurate but slower and bigger."
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
