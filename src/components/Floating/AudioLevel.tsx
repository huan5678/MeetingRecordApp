/**
 * AudioLevel — a horizontal peak meter (0..1). Used in the mini-panel and audio
 * settings to show mic / system-audio levels. Pure presentational.
 */

export interface AudioLevelProps {
  /** Normalised peak level, clamped to 0..1. */
  level: number;
  label?: string;
}

export function AudioLevel({ level, label }: AudioLevelProps) {
  const pct = Math.round(Math.min(1, Math.max(0, level)) * 100);
  // Green under 70%, amber to 90%, red above (clipping warning).
  const color =
    pct >= 90 ? "bg-recording" : pct >= 70 ? "bg-amber-500" : "bg-green-500";
  return (
    <div className="flex items-center gap-2">
      {label && (
        <span className="w-10 shrink-0 text-xs text-gray-500 dark:text-gray-400">
          {label}
        </span>
      )}
      <div
        role="meter"
        aria-label={label ? `${label} level` : "audio level"}
        aria-valuenow={pct}
        aria-valuemin={0}
        aria-valuemax={100}
        className="h-2 flex-1 overflow-hidden rounded-full bg-gray-200 dark:bg-gray-700"
      >
        <div
          className={`h-full ${color} transition-[width] duration-100`}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}
