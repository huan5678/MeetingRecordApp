/**
 * Recording-control hook. Selects the relevant slice of `recordingStore` and
 * exposes start/stop/pause/resume plus derived flags. Components should use this
 * rather than reaching into the store directly, so the wiring stays in one place.
 */

import { useRecordingStore, isActive } from "@/stores/recordingStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { RECORDING_STATES } from "@/lib/constants";

export interface UseRecordingResult {
  state: ReturnType<typeof useRecordingStore.getState>["state"];
  elapsedSeconds: number;
  micLevel: number;
  systemLevel: number;
  isRecording: boolean;
  isPaused: boolean;
  isActive: boolean;
  /** Start using the currently configured mic + system-audio setting. */
  start: () => Promise<void>;
  stop: () => Promise<string | null>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  /** Toggle start/stop — bound to the global record shortcut. */
  toggle: () => Promise<void>;
}

export function useRecording(): UseRecordingResult {
  const state = useRecordingStore((s) => s.state);
  const elapsedSeconds = useRecordingStore((s) => s.elapsedSeconds);
  const micLevel = useRecordingStore((s) => s.micLevel);
  const systemLevel = useRecordingStore((s) => s.systemLevel);
  const start = useRecordingStore((s) => s.start);
  const stop = useRecordingStore((s) => s.stop);
  const pause = useRecordingStore((s) => s.pause);
  const resume = useRecordingStore((s) => s.resume);

  const micDeviceId = useSettingsStore((s) => s.micDeviceId);
  const systemAudio = useSettingsStore((s) => s.systemAudioEnabled);

  const isRecording = state === RECORDING_STATES.Recording;
  const isPaused = state === RECORDING_STATES.Paused;

  const startWithSettings = () =>
    start({ micDeviceId: micDeviceId ?? undefined, systemAudio });

  const toggle = () =>
    isActive(state) ? stop().then(() => {}) : startWithSettings();

  return {
    state,
    elapsedSeconds,
    micLevel,
    systemLevel,
    isRecording,
    isPaused,
    isActive: isActive(state),
    start: startWithSettings,
    stop,
    pause,
    resume,
    toggle,
  };
}
