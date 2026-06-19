/**
 * Transcription-status hook. Polls the backend for the post-recording batch
 * transcription progress (PRD §3.3 #15 — progress indicator, NOT live).
 *
 * Only polls while the meeting is in a non-terminal state. In mock/browser mode
 * the backend has no running job, so this resolves to a `done` snapshot and
 * stops polling immediately.
 */

import { useEffect, useRef, useState } from "react";
import { api, type TranscriptionStatusDto } from "@/lib/tauri";
import { isTauri } from "@/lib/tauri";

const POLL_MS = 1500;
const TERMINAL: TranscriptionStatusDto["stage"][] = ["done", "error"];

export interface UseTranscriptionResult {
  status: TranscriptionStatusDto | null;
  loading: boolean;
  error: string | null;
}

export function useTranscription(
  meetingId: string | null,
  /** Set false to disable polling (e.g. when the detail view is hidden). */
  active = true,
): UseTranscriptionResult {
  const [status, setStatus] = useState<TranscriptionStatusDto | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (!meetingId || !active) {
      setStatus(null);
      return;
    }

    const clear = () => {
      if (timer.current != null) clearTimeout(timer.current);
      timer.current = null;
    };

    const poll = async () => {
      setLoading(true);
      try {
        const s = await api.getTranscriptionStatus(meetingId);
        if (cancelled) return;
        setStatus(s);
        setError(null);
        // Keep polling only inside Tauri and while non-terminal.
        if (isTauri() && !TERMINAL.includes(s.stage)) {
          timer.current = setTimeout(poll, POLL_MS);
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    void poll();
    return () => {
      cancelled = true;
      clear();
    };
  }, [meetingId, active]);

  return { status, loading, error };
}
