/**
 * SummaryView — renders an AI summary (PRD §3.4): markdown body, key decisions,
 * action items, plus provenance (provider/model/tokens). Includes a collapsible
 * regenerate panel (SummaryEditor). When no summary exists yet, shows a CTA.
 *
 * The markdown body is rendered with a tiny, dependency-free renderer (headings,
 * bullets, paragraphs) — sufficient for the structured summaries we produce and
 * keeps the bundle lean.
 */

import { useState } from "react";
import { ActionItems } from "@/components/Summary/ActionItems";
import { SummaryEditor } from "@/components/Summary/SummaryEditor";
import { Button } from "@/components/common/Button";
import { renderBasicMarkdown } from "@/lib/markdown";
import type { Summary } from "@/lib/types";

export interface SummaryViewProps {
  meetingId: string;
  summary: Summary | null;
  onRegenerated?: () => void;
}

export function SummaryView({
  meetingId,
  summary,
  onRegenerated,
}: SummaryViewProps) {
  const [editing, setEditing] = useState(false);

  if (!summary) {
    return (
      <div className="space-y-3">
        <p className="text-sm text-gray-500 dark:text-gray-400">
          No summary generated yet.
        </p>
        {editing ? (
          <SummaryEditor
            meetingId={meetingId}
            onGenerated={() => {
              setEditing(false);
              onRegenerated?.();
            }}
          />
        ) : (
          <Button onClick={() => setEditing(true)}>Generate summary</Button>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-5">
      <div className="flex items-start justify-between gap-3">
        <div
          className="prose-sm max-w-none text-gray-800 dark:text-gray-200"
          dangerouslySetInnerHTML={{
            __html: renderBasicMarkdown(summary.content),
          }}
        />
        <Button
          variant="secondary"
          size="sm"
          onClick={() => setEditing((v) => !v)}
        >
          {editing ? "Close" : "Regenerate"}
        </Button>
      </div>

      {summary.key_decisions.length > 0 && (
        <section>
          <h3 className="mb-2 text-sm font-semibold text-gray-900 dark:text-gray-100">
            Key Decisions
          </h3>
          <ol className="list-decimal space-y-1 pl-5 text-sm text-gray-800 dark:text-gray-200">
            {summary.key_decisions.map((d, i) => (
              <li key={i}>
                {d.decision}
                {d.context && (
                  <span className="text-gray-500 dark:text-gray-400">
                    {" "}
                    — {d.context}
                  </span>
                )}
              </li>
            ))}
          </ol>
        </section>
      )}

      <section>
        <h3 className="mb-2 text-sm font-semibold text-gray-900 dark:text-gray-100">
          Action Items
        </h3>
        <ActionItems items={summary.action_items} />
      </section>

      <footer className="text-xs text-gray-400 dark:text-gray-500">
        {summary.ai_provider && <span>via {summary.ai_provider}</span>}
        {summary.ai_model && <span> · {summary.ai_model}</span>}
        {summary.tokens_used != null && (
          <span> · {summary.tokens_used.toLocaleString()} tokens</span>
        )}
      </footer>

      {editing && (
        <SummaryEditor
          meetingId={meetingId}
          onGenerated={() => {
            setEditing(false);
            onRegenerated?.();
          }}
        />
      )}
    </div>
  );
}
