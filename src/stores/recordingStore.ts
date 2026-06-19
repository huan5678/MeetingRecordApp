/**
 * Recording + navigation state (Zustand).
 *
 * Owns the recording lifecycle (idle → recording ⇄ paused → stopping → idle),
 * the elapsed timer, live audio levels, and the main-window navigation (which
 * view + which meeting is selected). Actions call the backend via `api`; in
 * mock/browser mode the backend resolves instantly and we drive the timer
 * locally so the UI is fully interactive.
 */

import { create } from "zustand";
import {
  RECORDING_STATES,
  VIEWS,
  type RecordingState,
  type View,
} from "@/lib/constants";
import { api } from "@/lib/tauri";

export interface RecordingStoreState {
  // navigation
  view: View;
  selectedMeetingId: string | null;
  // recording
  state: RecordingState;
  meetingId: string | null;
  elapsedSeconds: number;
  micLevel: number;
  systemLevel: number;
  /** Whether the floating mini-panel is shown (during recording). */
  miniPanelOpen: boolean;
  /** id of the interval driving the local timer; internal. */
  _timer: ReturnType<typeof setInterval> | null;
}

interface RecordingActions {
  navigate: (view: View, meetingId?: string | null) => void;
  openMeeting: (meetingId: string) => void;
  start: (opts?: { micDeviceId?: string; systemAudio?: boolean }) => Promise<void>;
  stop: () => Promise<string | null>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  /** Apply a status snapshot pushed from the backend (poll/event). */
  applyStatus: (s: {
    state: RecordingState;
    meetingId: string | null;
    elapsedSeconds: number;
    micLevel: number;
    systemLevel: number;
  }) => void;
  /** Internal tick used by the local mock timer. */
  _tick: () => void;
}

export type RecordingStore = RecordingStoreState & RecordingActions;

const INITIAL: RecordingStoreState = {
  view: VIEWS.Meetings,
  selectedMeetingId: null,
  state: RECORDING_STATES.Idle,
  meetingId: null,
  elapsedSeconds: 0,
  micLevel: 0,
  systemLevel: 0,
  miniPanelOpen: false,
  _timer: null,
};

export const useRecordingStore = create<RecordingStore>((set, get) => ({
  ...INITIAL,

  navigate: (view, meetingId = null) =>
    set({ view, selectedMeetingId: meetingId ?? get().selectedMeetingId }),

  openMeeting: (meetingId) =>
    set({ view: VIEWS.Detail, selectedMeetingId: meetingId }),

  start: async (opts) => {
    if (get().state !== RECORDING_STATES.Idle) return;
    const meetingId = await api.startRecording({
      micDeviceId: opts?.micDeviceId,
      systemAudio: opts?.systemAudio ?? true,
    });
    set({
      state: RECORDING_STATES.Recording,
      meetingId,
      elapsedSeconds: 0,
      miniPanelOpen: true,
    });
    startTimer(set, get);
  },

  stop: async () => {
    if (get().state === RECORDING_STATES.Idle) return null;
    set({ state: RECORDING_STATES.Stopping });
    stopTimer(get, set);
    const id = await api.stopRecording().catch(() => get().meetingId);
    set({
      state: RECORDING_STATES.Idle,
      meetingId: null,
      elapsedSeconds: 0,
      micLevel: 0,
      systemLevel: 0,
      miniPanelOpen: false,
    });
    return id ?? null;
  },

  pause: async () => {
    if (get().state !== RECORDING_STATES.Recording) return;
    await api.pauseRecording().catch(() => {});
    set({ state: RECORDING_STATES.Paused });
    stopTimer(get, set);
  },

  resume: async () => {
    if (get().state !== RECORDING_STATES.Paused) return;
    await api.resumeRecording().catch(() => {});
    set({ state: RECORDING_STATES.Recording });
    startTimer(set, get);
  },

  applyStatus: (s) =>
    set({
      state: s.state,
      meetingId: s.meetingId,
      elapsedSeconds: s.elapsedSeconds,
      micLevel: s.micLevel,
      systemLevel: s.systemLevel,
      miniPanelOpen:
        s.state === RECORDING_STATES.Recording ||
        s.state === RECORDING_STATES.Paused,
    }),

  _tick: () => {
    const { state } = get();
    if (state !== RECORDING_STATES.Recording) return;
    // Synthetic levels in mock mode; real levels arrive via applyStatus in Tauri.
    set((st) => ({
      elapsedSeconds: st.elapsedSeconds + 1,
      micLevel: 0.25 + Math.random() * 0.5,
      systemLevel: 0.2 + Math.random() * 0.4,
    }));
  },
}));

function startTimer(
  set: (partial: Partial<RecordingStoreState>) => void,
  get: () => RecordingStore,
) {
  if (typeof setInterval === "undefined") return;
  stopTimer(get, set);
  const timer = setInterval(() => get()._tick(), 1000);
  set({ _timer: timer });
}

function stopTimer(
  get: () => RecordingStore,
  set: (partial: Partial<RecordingStoreState>) => void,
) {
  const t = get()._timer;
  if (t != null) {
    clearInterval(t);
    set({ _timer: null });
  }
}

/** True when actively recording or paused (drives tray accent + mini-panel). */
export function isActive(state: RecordingState): boolean {
  return (
    state === RECORDING_STATES.Recording || state === RECORDING_STATES.Paused
  );
}
