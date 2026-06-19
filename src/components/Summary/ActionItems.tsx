/**
 * ActionItems — checklist of extracted follow-ups (PRD §3.4 #19). Each item has
 * a task, optional owner + deadline, and a UI-side `done` toggle. Toggling is
 * local state in the skeleton; the Integrate phase persists it.
 */

import { useState } from "react";
import type { ActionItem } from "@/lib/types";

export interface ActionItemsProps {
  items: ActionItem[];
}

export function ActionItems({ items }: ActionItemsProps) {
  const [done, setDone] = useState<Record<number, boolean>>(() =>
    Object.fromEntries(items.map((it, i) => [i, it.done])),
  );

  if (items.length === 0) {
    return (
      <p className="text-sm text-gray-500 dark:text-gray-400">
        No action items extracted.
      </p>
    );
  }

  return (
    <ul className="flex flex-col gap-2">
      {items.map((it, i) => (
        <li key={i} className="flex items-start gap-2">
          <input
            type="checkbox"
            checked={!!done[i]}
            onChange={(e) => setDone((d) => ({ ...d, [i]: e.target.checked }))}
            className="mt-1 h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-800"
            aria-label={it.task}
          />
          <div className="text-sm">
            <span
              className={
                done[i]
                  ? "text-gray-400 line-through dark:text-gray-500"
                  : "text-gray-800 dark:text-gray-200"
              }
            >
              {it.task}
            </span>
            <span className="ml-2 text-xs text-gray-500 dark:text-gray-400">
              {it.owner && <span>@{it.owner}</span>}
              {it.owner && it.deadline && <span> · </span>}
              {it.deadline && <span>due {it.deadline}</span>}
            </span>
          </div>
        </li>
      ))}
    </ul>
  );
}
