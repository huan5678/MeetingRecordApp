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
  // Monochrome: full ink signals clipping (>=90%), muted otherwise.
  const color = pct >= 90 ? "bg-fg" : "bg-muted";
  return (
    <div className="flex items-center gap-2.5">
      {label && <span className="eyebrow w-8 shrink-0 text-faint">{label}</span>}
      <div
        role="meter"
        aria-label={label ? `${label} level` : "audio level"}
        aria-valuenow={pct}
        aria-valuemin={0}
        aria-valuemax={100}
        className="h-1.5 flex-1 overflow-hidden bg-line"
      >
        <div
          className={`h-full ${color} transition-[width] duration-100`}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}
