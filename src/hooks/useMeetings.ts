/**
 * Data hooks for the meeting history list and a single meeting's detail.
 * Thin async wrappers over `api.*` with loading/error state. No external data
 * library to keep the dependency surface minimal (PRD: surgical).
 */

import { useCallback, useEffect, useState } from "react";
import { api, type MeetingDetailDto } from "@/lib/tauri";
import type { Meeting } from "@/lib/types";

export interface UseMeetingsResult {
  meetings: Meeting[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  search: (query: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
}

/** List + search + delete over the meeting history. */
export function useMeetings(): UseMeetingsResult {
  const [meetings, setMeetings] = useState<Meeting[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setMeetings(await api.listMeetings());
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const search = useCallback(
    async (query: string) => {
      const q = query.trim();
      if (!q) return refresh();
      setLoading(true);
      setError(null);
      try {
        setMeetings(await api.searchTranscripts(q));
      } catch (e) {
        setError(errMsg(e));
      } finally {
        setLoading(false);
      }
    },
    [refresh],
  );

  const remove = useCallback(async (id: string) => {
    await api.deleteMeeting(id).catch(() => {});
    setMeetings((prev) => prev.filter((m) => m.id !== id));
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { meetings, loading, error, refresh, search, remove };
}

export interface UseMeetingDetailResult {
  detail: MeetingDetailDto | null;
  loading: boolean;
  error: string | null;
  reload: () => Promise<void>;
}

/** Load the player + transcript + summary bundle for one meeting. */
export function useMeetingDetail(
  meetingId: string | null,
): UseMeetingDetailResult {
  const [detail, setDetail] = useState<MeetingDetailDto | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!meetingId) {
      setDetail(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      setDetail(await api.getMeetingDetail(meetingId));
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setLoading(false);
    }
  }, [meetingId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  return { detail, loading, error, reload };
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
