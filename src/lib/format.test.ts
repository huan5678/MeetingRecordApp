import { describe, it, expect } from "vitest";
import {
  formatDuration,
  formatClock,
  formatTimestamp,
  formatBytes,
  formatDateTime,
} from "@/lib/format";

describe("formatDuration", () => {
  it("formats hours and minutes", () => {
    expect(formatDuration(5400)).toBe("1h 30m");
  });
  it("formats minutes and seconds", () => {
    expect(formatDuration(90)).toBe("1m 30s");
  });
  it("formats sub-minute as seconds", () => {
    expect(formatDuration(42)).toBe("42s");
  });
  it("returns em-dash for null/negative", () => {
    expect(formatDuration(null)).toBe("—");
    expect(formatDuration(undefined)).toBe("—");
    expect(formatDuration(-5)).toBe("—");
  });
});

describe("formatClock", () => {
  it("pads mm:ss under an hour", () => {
    expect(formatClock(65)).toBe("01:05");
  });
  it("includes hours when >= 1h", () => {
    expect(formatClock(3725)).toBe("01:02:05");
  });
  it("clamps negatives to zero", () => {
    expect(formatClock(-3)).toBe("00:00");
  });
});

describe("formatTimestamp", () => {
  it("converts ms to mm:ss", () => {
    expect(formatTimestamp(4200)).toBe("00:04");
    expect(formatTimestamp(125_000)).toBe("02:05");
  });
});

describe("formatBytes", () => {
  it("formats MB with one decimal", () => {
    expect(formatBytes(103_809_024)).toBe("99.0 MB");
  });
  it("formats bytes with no decimal", () => {
    expect(formatBytes(512)).toBe("512 B");
  });
  it("handles zero and null", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(null)).toBe("—");
  });
});

describe("formatDateTime", () => {
  it("returns em-dash for empty", () => {
    expect(formatDateTime(null)).toBe("—");
    expect(formatDateTime("")).toBe("—");
  });
  it("passes through unparseable strings", () => {
    expect(formatDateTime("not-a-date")).toBe("not-a-date");
  });
  it("formats a valid ISO timestamp to a non-empty string", () => {
    const out = formatDateTime("2026-06-18T14:00:00Z");
    expect(out).not.toBe("—");
    expect(out.length).toBeGreaterThan(0);
  });
  it("treats a zone-less backend timestamp as UTC (same instant as the Z form)", () => {
    // The Rust side stores "YYYY-MM-DD HH:MM:SS" in UTC, no zone marker.
    expect(formatDateTime("2026-06-18 14:00:00")).toBe(
      formatDateTime("2026-06-18T14:00:00Z"),
    );
  });
});
