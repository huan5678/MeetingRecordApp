/**
 * AudioSettings — microphone + system-audio configuration (PRD §3.8 #43).
 * Lists input devices (cpal), toggles WASAPI system-audio loopback (Windows
 * v1.0) and the optional dual-track keep for diarization (PRD §4.4).
 */

import { useEffect, useState } from "react";
import { api, type AudioDevice } from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settingsStore";
import { AudioLevel } from "@/components/Floating/AudioLevel";
import { useRecording } from "@/hooks/useRecording";
import { Field, Row, Toggle } from "@/components/Settings/controls";

export function AudioSettings() {
  const micDeviceId = useSettingsStore((s) => s.micDeviceId);
  const systemAudioEnabled = useSettingsStore((s) => s.systemAudioEnabled);
  const keepDualTrack = useSettingsStore((s) => s.keepDualTrack);
  const setField = useSettingsStore((s) => s.setField);

  const rec = useRecording();
  const [devices, setDevices] = useState<AudioDevice[]>([]);

  useEffect(() => {
    void api
      .listAudioDevices()
      .then(setDevices)
      .catch(() => setDevices([]));
  }, []);

  const inputs = devices.filter((d) => d.kind === "input");

  return (
    <div className="space-y-5">
      <Field label="Microphone" hint="Captured via cpal.">
        <select
          value={micDeviceId ?? ""}
          onChange={(e) => setField("micDeviceId", e.target.value || null)}
          className="block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
        >
          <option value="">System default</option>
          {inputs.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name}
              {d.isDefault ? " (default)" : ""}
            </option>
          ))}
        </select>
      </Field>

      <Field
        label="Input level"
        hint="Live peak meter (active while recording)."
      >
        <AudioLevel level={rec.micLevel} />
      </Field>

      <Row
        label="Capture system audio"
        hint="WASAPI loopback — Windows only in v1.0."
      >
        <Toggle
          checked={systemAudioEnabled}
          onChange={(v) => setField("systemAudioEnabled", v)}
        />
      </Row>

      <Row
        label="Keep dual track"
        hint="Save mic + system separately to aid diarization (larger files)."
      >
        <Toggle
          checked={keepDualTrack}
          onChange={(v) => setField("keepDualTrack", v)}
        />
      </Row>
    </div>
  );
}
