import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { useRecordingStore, isActive } from "@/stores/recordingStore";
import { RECORDING_STATES, VIEWS } from "@/lib/constants";

function reset() {
  // Stop any timer the previous test left running, then reset state.
  const t = useRecordingStore.getState()._timer;
  if (t != null) clearInterval(t);
  useRecordingStore.setState({
    view: VIEWS.Meetings,
    selectedMeetingId: null,
    state: RECORDING_STATES.Idle,
    meetingId: null,
    elapsedSeconds: 0,
    micLevel: 0,
    systemLevel: 0,
    miniPanelOpen: false,
    _timer: null,
  });
}

describe("recordingStore", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    reset();
  });
  afterEach(() => {
    reset();
    vi.useRealTimers();
  });

  it("starts recording (mock backend) and opens the mini-panel", async () => {
    await useRecordingStore.getState().start({ systemAudio: true });
    const s = useRecordingStore.getState();
    expect(s.state).toBe(RECORDING_STATES.Recording);
    expect(s.meetingId).toBeTruthy();
    expect(s.miniPanelOpen).toBe(true);
  });

  it("advances the elapsed timer while recording", async () => {
    await useRecordingStore.getState().start();
    vi.advanceTimersByTime(3000);
    expect(useRecordingStore.getState().elapsedSeconds).toBe(3);
  });

  it("pauses and resumes, gating the timer", async () => {
    await useRecordingStore.getState().start();
    vi.advanceTimersByTime(2000);
    await useRecordingStore.getState().pause();
    expect(useRecordingStore.getState().state).toBe(RECORDING_STATES.Paused);
    const atPause = useRecordingStore.getState().elapsedSeconds;
    vi.advanceTimersByTime(5000);
    // No advance while paused.
    expect(useRecordingStore.getState().elapsedSeconds).toBe(atPause);
    await useRecordingStore.getState().resume();
    vi.advanceTimersByTime(1000);
    expect(useRecordingStore.getState().elapsedSeconds).toBe(atPause + 1);
  });

  it("stops recording and clears state", async () => {
    await useRecordingStore.getState().start();
    const id = await useRecordingStore.getState().stop();
    const s = useRecordingStore.getState();
    expect(id).toBeTruthy();
    expect(s.state).toBe(RECORDING_STATES.Idle);
    expect(s.meetingId).toBeNull();
    expect(s.miniPanelOpen).toBe(false);
    expect(s.elapsedSeconds).toBe(0);
  });

  it("openMeeting navigates to the detail view", () => {
    useRecordingStore.getState().openMeeting("mtg-001");
    const s = useRecordingStore.getState();
    expect(s.view).toBe(VIEWS.Detail);
    expect(s.selectedMeetingId).toBe("mtg-001");
  });

  it("isActive reflects recording/paused", () => {
    expect(isActive(RECORDING_STATES.Idle)).toBe(false);
    expect(isActive(RECORDING_STATES.Recording)).toBe(true);
    expect(isActive(RECORDING_STATES.Paused)).toBe(true);
  });
});
