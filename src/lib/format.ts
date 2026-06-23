/**
 * Pure formatting helpers shared across components. No React, no I/O — trivially
 * unit-testable (see format.test.ts).
 */

/** `90` -> `"1m 30s"`, `5400` -> `"1h 30m"`, `null` -> `"—"`. */
export function formatDuration(totalSeconds: number | null | undefined): string {
  if (totalSeconds == null || totalSeconds < 0) return "—";
  const s = Math.floor(totalSeconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${sec}s`;
  return `${sec}s`;
}

/** `3725` -> `"01:02:05"` — fixed clock format for the recording timer / player. */
export function formatClock(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${pad(h)}:${pad(m)}:${pad(sec)}` : `${pad(m)}:${pad(sec)}`;
}

/** Milliseconds -> `"mm:ss"` for transcript segment timestamps. */
export function formatTimestamp(ms: number): string {
  return formatClock(Math.floor(ms / 1000));
}

/** Human-readable file size. `103809024` -> `"99.0 MB"`. */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || bytes < 0) return "—";
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(
    units.length - 1,
    Math.floor(Math.log(bytes) / Math.log(1024)),
  );
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

/** A meeting's display name: its title, or its start time when unnamed. */
export function meetingTitle(
  title: string | null | undefined,
  startTime: string,
): string {
  const t = title?.trim();
  return t ? t : formatDateTime(startTime);
}

/**
 * Parse a backend timestamp into a Date. The Rust side stores UTC wall-clock
 * with NO zone marker (e.g. "2026-06-23 09:48:18"); `new Date()` would read that
 * as *local* time and be off by the user's offset. So when the string carries
 * no zone (trailing `Z` or `±HH:MM`), treat it as UTC.
 */
function parseBackendDate(iso: string): Date {
  const hasZone = /(Z|[+-]\d{2}:?\d{2})$/.test(iso);
  return new Date(hasZone ? iso : `${iso.replace(" ", "T")}Z`);
}

/** ISO timestamp -> locale date+time, gracefully degrading on bad input. */
export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = parseBackendDate(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}
