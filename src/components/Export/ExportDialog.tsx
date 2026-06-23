/**
 * ExportDialog — choose a format and save a meeting to a file the user picks
 * (PRD §3.6 / §4.8). v1.0 formats: Markdown, SRT, VTT, JSON. Opens a native Save
 * dialog, then `api.exportMeeting` writes the rendered file to that path (in
 * mock/browser mode it falls back to the backend's default location).
 */

import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Modal } from "@/components/common/Modal";
import { Button } from "@/components/common/Button";
import { api, isTauri } from "@/lib/tauri";
import { EXPORT_FORMATS, type ExportFormat } from "@/lib/constants";

export interface ExportDialogProps {
  open: boolean;
  meetingId: string;
  /** Used as the suggested filename in the Save dialog. */
  title?: string;
  onClose: () => void;
}

/** Strip characters that are illegal in filenames on Windows/macOS. */
function safeName(name: string): string {
  return name.replace(/[\\/:*?"<>|]/g, "_").trim() || "meeting";
}

export function ExportDialog({ open, meetingId, title, onClose }: ExportDialogProps) {
  const [format, setFormat] = useState<ExportFormat>("markdown");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  const doExport = async () => {
    setResult(null);
    const fmt = EXPORT_FORMATS.find((f) => f.id === format);
    if (!fmt) return;

    setBusy(true);
    try {
      let dest: string | undefined;
      if (isTauri()) {
        const picked = await save({
          defaultPath: `${safeName(title ?? "meeting")}.${fmt.ext}`,
          filters: [{ name: fmt.label, extensions: [fmt.ext] }],
        });
        if (!picked) {
          setBusy(false);
          return; // user cancelled
        }
        dest = picked;
      }
      const path = await api.exportMeeting({ meetingId, format, dest });
      setResult(`Saved to ${path}`);
    } catch (e) {
      setResult(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      title="Export meeting"
      onClose={onClose}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
          <Button onClick={() => void doExport()} disabled={busy}>
            {busy ? "Saving…" : "Save as…"}
          </Button>
        </>
      }
    >
      <fieldset className="space-y-1">
        <legend className="eyebrow mb-2">Format</legend>
        {EXPORT_FORMATS.map((f) => (
          <label
            key={f.id}
            className="flex cursor-pointer items-center gap-3 py-1.5 text-sm text-fg"
          >
            <input
              type="radio"
              name="export-format"
              value={f.id}
              checked={format === f.id}
              onChange={() => setFormat(f.id)}
              className="h-3.5 w-3.5 accent-fg"
            />
            {f.label}
          </label>
        ))}
      </fieldset>

      {result && (
        <p className="num mt-4 break-all border border-line bg-surface p-2.5 text-[11px] text-muted">
          {result}
        </p>
      )}
    </Modal>
  );
}
