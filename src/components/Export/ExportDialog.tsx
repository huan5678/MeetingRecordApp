/**
 * ExportDialog — choose a format and export a meeting (PRD §3.6 / §4.8).
 * v1.0 formats: Markdown, SRT, VTT, JSON (PDF is v1.1). Calls
 * `api.exportMeeting`, which returns the written file path (or the content in
 * mock mode).
 */

import { useState } from "react";
import { Modal } from "@/components/common/Modal";
import { Button } from "@/components/common/Button";
import { api } from "@/lib/tauri";
import { EXPORT_FORMATS, type ExportFormat } from "@/lib/constants";

export interface ExportDialogProps {
  open: boolean;
  meetingId: string;
  onClose: () => void;
}

export function ExportDialog({ open, meetingId, onClose }: ExportDialogProps) {
  const [format, setFormat] = useState<ExportFormat>("markdown");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  const doExport = async () => {
    setBusy(true);
    setResult(null);
    try {
      const path = await api.exportMeeting({ meetingId, format });
      setResult(path);
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
            {busy ? "Exporting…" : "Export"}
          </Button>
        </>
      }
    >
      <fieldset className="space-y-2">
        <legend className="mb-1 text-xs font-medium text-gray-500 dark:text-gray-400">
          Format
        </legend>
        {EXPORT_FORMATS.map((f) => (
          <label key={f.id} className="flex items-center gap-2">
            <input
              type="radio"
              name="export-format"
              value={f.id}
              checked={format === f.id}
              onChange={() => setFormat(f.id)}
              className="h-4 w-4 text-blue-600 focus:ring-blue-500"
            />
            <span className="text-sm text-gray-800 dark:text-gray-200">
              {f.label}
            </span>
          </label>
        ))}
      </fieldset>

      {result && (
        <p className="mt-3 break-all rounded bg-gray-100 p-2 text-xs text-gray-600 dark:bg-gray-800 dark:text-gray-300">
          {result}
        </p>
      )}
    </Modal>
  );
}
