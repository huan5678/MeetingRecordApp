/**
 * Tooltip — CSS-only hover/focus tooltip. No positioning library; uses a
 * group-hover wrapper so it works for keyboard focus too. Good enough for the
 * v1.0 controls (shortcuts hints, icon buttons).
 */

import type { ReactNode } from "react";

export interface TooltipProps {
  label: string;
  children: ReactNode;
  /** Where the bubble appears relative to the trigger. */
  side?: "top" | "bottom";
}

export function Tooltip({ label, children, side = "top" }: TooltipProps) {
  const pos =
    side === "top"
      ? "bottom-full mb-1.5"
      : "top-full mt-1.5";
  return (
    <span className="group relative inline-flex">
      {children}
      <span
        role="tooltip"
        className={
          `pointer-events-none absolute left-1/2 z-50 -translate-x-1/2 ${pos} ` +
          "whitespace-nowrap rounded bg-gray-900 px-2 py-1 text-xs text-white " +
          "opacity-0 shadow transition-opacity group-hover:opacity-100 " +
          "group-focus-within:opacity-100 dark:bg-gray-700"
        }
      >
        {label}
      </span>
    </span>
  );
}
