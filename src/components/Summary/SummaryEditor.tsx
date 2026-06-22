/**
 * SummaryEditor — custom-prompt + provider/model picker for (re)generating a
 * summary (PRD §3.4 #22). Shows a cloud-cost estimate before sending to a cloud
 * provider; Ollama (local, default) has no cost (PRD §4.6).
 */

import { useState } from "react";
import { Button } from "@/components/common/Button";
import { api, type CostEstimateDto } from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settingsStore";
import {
  PROVIDER_META,
  type AiProvider,
  AI_PROVIDERS,
} from "@/lib/constants";

export interface SummaryEditorProps {
  meetingId: string;
  onGenerated?: () => void;
}

export function SummaryEditor({ meetingId, onGenerated }: SummaryEditorProps) {
  const defaultProvider = useSettingsStore((s) => s.aiProvider);
  const defaultModel = useSettingsStore((s) => s.aiModel);

  const [provider, setProvider] = useState<AiProvider>(defaultProvider);
  const [model, setModel] = useState(defaultModel);
  const [prompt, setPrompt] = useState("");
  const [estimate, setEstimate] = useState<CostEstimateDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isCloud = provider !== AI_PROVIDERS.Ollama;

  const changeProvider = (p: AiProvider) => {
    setProvider(p);
    setModel(PROVIDER_META[p].models[0]);
    setEstimate(null);
  };

  const estimateCost = async () => {
    const e = await api
      .estimateSummaryCost({ meetingId, provider, model })
      .catch(() => null);
    setEstimate(e);
  };

  const generate = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.generateSummary({
        meetingId,
        provider,
        model,
        prompt: prompt.trim() || undefined,
      });
      onGenerated?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-3 rounded-lg border border-gray-200 bg-white p-4 dark:border-gray-800 dark:bg-gray-900">
      <div className="grid grid-cols-2 gap-3">
        <label className="text-xs text-gray-600 dark:text-gray-400">
          Provider
          <select
            value={provider}
            onChange={(e) => changeProvider(e.target.value as AiProvider)}
            className="mt-1 block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          >
            {Object.values(PROVIDER_META).map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
        </label>
        <label className="text-xs text-gray-600 dark:text-gray-400">
          Model
          <select
            value={model}
            onChange={(e) => setModel(e.target.value)}
            className="mt-1 block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          >
            {PROVIDER_META[provider].models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </label>
      </div>

      <label className="block text-xs text-gray-600 dark:text-gray-400">
        Custom prompt (optional)
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          rows={3}
          placeholder="e.g. Focus on decisions and risks; list owners for each action item."
          className="mt-1 block w-full resize-y rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
        />
      </label>

      {isCloud && (
        <div className="rounded-md bg-amber-50 p-2 text-xs text-amber-800 dark:bg-amber-900/30 dark:text-amber-300">
          This transcript will be sent to a cloud provider.
          {estimate ? (
            <span className="ml-1">
              Est. {estimate.promptTokens.toLocaleString()} tokens ≈ $
              {estimate.estimatedUsd.toFixed(3)}.
            </span>
          ) : (
            <button
              type="button"
              onClick={() => void estimateCost()}
              className="ml-1 underline"
            >
              Estimate cost
            </button>
          )}
        </div>
      )}

      {error && (
        <p
          className="rounded-md bg-red-50 p-2 text-xs text-red-700 dark:bg-red-900/30 dark:text-red-300"
          role="alert"
        >
          {error.includes("no transcript")
            ? "This meeting has no transcript yet — transcription needs a build with the whisper feature + a downloaded model. Summary needs a transcript to work from."
            : error}
        </p>
      )}

      <div className="flex justify-end">
        <Button onClick={() => void generate()} disabled={busy}>
          {busy ? "Generating…" : "Generate summary"}
        </Button>
      </div>
    </div>
  );
}
