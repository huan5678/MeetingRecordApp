/**
 * MeetingDetail — the single-meeting view (PRD §3.5 #28): player + transcript +
 * summary together, with tabs to switch focus, plus Export and Back actions.
 *
 * A meeting can be transcribed/summarized more than once (different models), and
 * every result is kept. The transcript tab offers a run selector (by model +
 * time) and a "re-transcribe with another model" panel; the summary tab offers
 * a summary selector. Older versions can be deleted.
 */

import { useEffect, useState } from "react";
import { useMeetingDetail } from "@/hooks/useMeetings";
import { useTranscription } from "@/hooks/useTranscription";
import { useRecordingStore } from "@/stores/recordingStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { MeetingPlayer } from "@/components/Meeting/MeetingPlayer";
import { Transcript } from "@/components/Meeting/Transcript";
import { SummaryView } from "@/components/Summary/SummaryView";
import { ExportDialog } from "@/components/Export/ExportDialog";
import { Button } from "@/components/common/Button";
import { api } from "@/lib/tauri";
import { formatDateTime, formatDuration, meetingTitle } from "@/lib/format";
import {
  GEMINI_TRANSCRIBE_MODELS,
  MEETING_STATUS,
  MEETING_TYPE_LABELS,
  TRANSCRIPTION_ENGINES,
  VIEWS,
  WHISPER_MODELS,
} from "@/lib/constants";
import type { Summary, TranscriptRun, TranscriptSegment } from "@/lib/types";

type Tab = "summary" | "transcript";

export function MeetingDetail() {
  const meetingId = useRecordingStore((s) => s.selectedMeetingId);
  const navigate = useRecordingStore((s) => s.navigate);
  const { detail, loading, error, reload } = useMeetingDetail(meetingId);
  // `txNonce` restarts the status poll after a re-transcribe of an already
  // settled meeting (whose poll loop had stopped at a terminal stage).
  const [txNonce, setTxNonce] = useState(0);
  const transcription = useTranscription(meetingId, !!detail, txNonce);
  const [tab, setTab] = useState<Tab>("summary");
  const [exporting, setExporting] = useState(false);
  const [titleEditing, setTitleEditing] = useState(false);
  const [titleDraft, setTitleDraft] = useState("");

  const onTranscribeStarted = () => {
    setTxNonce((n) => n + 1);
    void reload();
  };

  // Reload the detail when a (re)transcription settles so the new run/summary
  // (or the error) is reflected without a manual refresh.
  const stage = transcription.status?.stage;
  useEffect(() => {
    if (stage === "done" || stage === "error") void reload();
  }, [stage, reload]);

  if (!meetingId) {
    return (
      <Empty onBack={() => navigate(VIEWS.Meetings)} message="No meeting selected." />
    );
  }
  if (loading) {
    return <Empty onBack={() => navigate(VIEWS.Meetings)} message="Loading…" />;
  }
  if (error || !detail) {
    return (
      <Empty
        onBack={() => navigate(VIEWS.Meetings)}
        message={error ?? "Meeting not found."}
      />
    );
  }

  const { meeting, media, segments, runs, summaries } = detail;

  const beginEditTitle = () => {
    setTitleDraft(meeting.title ?? "");
    setTitleEditing(true);
  };
  const saveTitle = async () => {
    setTitleEditing(false);
    await api.updateMeeting(meeting.id, { title: titleDraft.trim() }).catch(() => {});
    void reload();
  };

  const stillTranscribing =
    meeting.status === MEETING_STATUS.Transcribing ||
    transcription.status?.stage === "transcribing" ||
    transcription.status?.stage === "diarizing";

  return (
    <div className="mx-auto flex h-full w-full max-w-4xl flex-col gap-4 p-6">
      <div className="flex items-start justify-between gap-3">
        <div>
          <Button
            variant="ghost"
            size="sm"
            className="-ml-2 mb-1"
            onClick={() => navigate(VIEWS.Meetings)}
          >
            ← History
          </Button>
          {titleEditing ? (
            <input
              autoFocus
              value={titleDraft}
              placeholder={formatDateTime(meeting.start_time)}
              onChange={(e) => setTitleDraft(e.target.value)}
              onBlur={() => void saveTitle()}
              onKeyDown={(e) => {
                if (e.key === "Enter") void saveTitle();
                if (e.key === "Escape") setTitleEditing(false);
              }}
              className="w-full rounded-md border border-gray-300 bg-white px-2 py-1 text-xl font-semibold text-gray-900 focus:border-blue-500 focus:outline-none dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
            />
          ) : (
            <h1
              onClick={beginEditTitle}
              title="點擊以重新命名"
              className="cursor-text text-xl font-semibold text-gray-900 hover:opacity-80 dark:text-gray-100"
            >
              {meetingTitle(meeting.title, meeting.start_time)}
              <span className="ml-2 align-middle text-xs font-normal text-gray-400">
                ✎
              </span>
            </h1>
          )}
          <p className="text-sm text-gray-500 dark:text-gray-400">
            {formatDateTime(meeting.start_time)} ·{" "}
            {formatDuration(meeting.duration_seconds)}
            {meeting.meeting_type &&
              ` · ${MEETING_TYPE_LABELS[meeting.meeting_type]}`}
          </p>
        </div>
        <Button variant="secondary" onClick={() => setExporting(true)}>
          Export
        </Button>
      </div>

      <MeetingPlayer media={media} />

      {stillTranscribing && (
        <div className="rounded-md bg-amber-50 px-3 py-2 text-sm text-amber-800 dark:bg-amber-900/30 dark:text-amber-300">
          Transcribing…{" "}
          {transcription.status
            ? `${Math.round(transcription.status.progress * 100)}%`
            : ""}
        </div>
      )}

      {transcription.status?.stage === "error" && (
        <div
          className="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700 dark:bg-red-900/30 dark:text-red-300"
          role="alert"
        >
          轉錄失敗:{transcription.status.message ?? "未知錯誤"}
        </div>
      )}

      <div className="flex gap-1 border-b border-gray-200 dark:border-gray-800">
        <TabButton active={tab === "summary"} onClick={() => setTab("summary")}>
          Summary
        </TabButton>
        <TabButton
          active={tab === "transcript"}
          onClick={() => setTab("transcript")}
        >
          Transcript
        </TabButton>
      </div>

      <div className="flex-1 overflow-auto pb-6">
        {tab === "summary" ? (
          <SummaryTab
            meetingId={meeting.id}
            summaries={summaries}
            onChanged={() => void reload()}
          />
        ) : (
          <TranscriptTab
            meetingId={meeting.id}
            latestSegments={segments}
            runs={runs}
            readOnlyBase={stillTranscribing}
            onChanged={() => void reload()}
            onTranscribeStarted={onTranscribeStarted}
          />
        )}
      </div>

      <ExportDialog
        open={exporting}
        meetingId={meeting.id}
        onClose={() => setExporting(false)}
      />
    </div>
  );
}

const runLabel = (r: TranscriptRun) =>
  `${r.model} · ${formatDateTime(r.created_at)} · ${r.segment_count} 段`;

const summaryLabel = (s: Summary) =>
  `${s.ai_model ?? s.ai_provider ?? "summary"} · ${formatDateTime(s.created_at)}`;

/** Transcript tab: run selector + re-transcribe panel + the segments. */
function TranscriptTab({
  meetingId,
  latestSegments,
  runs,
  readOnlyBase,
  onChanged,
  onTranscribeStarted,
}: {
  meetingId: string;
  latestSegments: TranscriptSegment[];
  runs: TranscriptRun[];
  readOnlyBase: boolean;
  onChanged: () => void;
  onTranscribeStarted: () => void;
}) {
  const latestRunId = runs[0]?.id ?? null;
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [runSegments, setRunSegments] = useState<TranscriptSegment[] | null>(null);

  const isLatest = !selectedRunId || selectedRunId === latestRunId;
  const shown = isLatest ? latestSegments : runSegments ?? [];

  const selectRun = async (runId: string) => {
    setSelectedRunId(runId);
    if (!runId || runId === latestRunId) {
      setRunSegments(null);
      return;
    }
    setRunSegments(await api.getRunSegments(runId).catch(() => []));
  };

  const deleteRun = async (runId: string) => {
    await api.deleteTranscriptRun(runId).catch(() => {});
    setSelectedRunId(null);
    setRunSegments(null);
    onChanged();
  };

  return (
    <div className="space-y-3">
      {runs.length > 1 && (
        <div className="flex items-center gap-2">
          <label className="text-xs text-gray-500 dark:text-gray-400">版本</label>
          <select
            value={selectedRunId ?? latestRunId ?? ""}
            onChange={(e) => void selectRun(e.target.value)}
            className="rounded-md border border-gray-300 bg-white p-1.5 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          >
            {runs.map((r) => (
              <option key={r.id} value={r.id}>
                {runLabel(r)}
              </option>
            ))}
          </select>
          {!isLatest && selectedRunId && (
            <button
              type="button"
              onClick={() => void deleteRun(selectedRunId)}
              className="text-xs text-red-600 hover:underline dark:text-red-400"
            >
              刪除此版本
            </button>
          )}
        </div>
      )}

      <RetranscribePanel meetingId={meetingId} onStarted={onTranscribeStarted} />

      <Transcript segments={shown} readOnly={readOnlyBase || !isLatest} />
    </div>
  );
}

/** Summary tab: summary selector + the selected summary. */
function SummaryTab({
  meetingId,
  summaries,
  onChanged,
}: {
  meetingId: string;
  summaries: Summary[];
  onChanged: () => void;
}) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected =
    summaries.find((s) => s.id === selectedId) ?? summaries[0] ?? null;

  const deleteSummary = async (id: string) => {
    await api.deleteSummary(id).catch(() => {});
    setSelectedId(null);
    onChanged();
  };

  return (
    <div className="space-y-3">
      {summaries.length > 1 && (
        <div className="flex items-center gap-2">
          <label className="text-xs text-gray-500 dark:text-gray-400">版本</label>
          <select
            value={selected?.id ?? ""}
            onChange={(e) => setSelectedId(e.target.value)}
            className="rounded-md border border-gray-300 bg-white p-1.5 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          >
            {summaries.map((s) => (
              <option key={s.id} value={s.id}>
                {summaryLabel(s)}
              </option>
            ))}
          </select>
          {selected && (
            <button
              type="button"
              onClick={() => void deleteSummary(selected.id)}
              className="text-xs text-red-600 hover:underline dark:text-red-400"
            >
              刪除此版本
            </button>
          )}
        </div>
      )}
      <SummaryView
        meetingId={meetingId}
        summary={selected}
        onRegenerated={onChanged}
      />
    </div>
  );
}

/** Collapsible "re-transcribe with another model" control. */
function RetranscribePanel({
  meetingId,
  onStarted,
}: {
  meetingId: string;
  onStarted: () => void;
}) {
  const defaultEngine = useSettingsStore((s) => s.transcriptionEngine);
  const defaultGeminiModel = useSettingsStore((s) => s.geminiModel);
  const defaultWhisperModel = useSettingsStore((s) => s.whisperModel);
  const defaultLanguage = useSettingsStore((s) => s.language);
  const [open, setOpen] = useState(false);
  const [engine, setEngine] = useState(defaultEngine);
  const [geminiModel, setGeminiModel] = useState(defaultGeminiModel);
  const [whisperModel, setWhisperModel] = useState<string>(defaultWhisperModel);
  const [busy, setBusy] = useState(false);

  const usesGemini = engine !== "whisper";

  const run = async () => {
    setBusy(true);
    try {
      await api.retranscribeMeeting(meetingId, {
        engine,
        geminiModel,
        whisperModel,
        language: defaultLanguage,
      });
      setOpen(false);
      onStarted();
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="text-sm text-blue-600 hover:underline dark:text-blue-400"
      >
        用其他模型重新轉錄 ▾
      </button>
    );
  }

  return (
    <div className="space-y-3 rounded-lg border border-gray-200 bg-white p-4 dark:border-gray-800 dark:bg-gray-900">
      <div className="grid grid-cols-2 gap-3">
        <label className="text-xs text-gray-600 dark:text-gray-400">
          引擎
          <select
            value={engine}
            onChange={(e) => setEngine(e.target.value as typeof engine)}
            className="mt-1 block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
          >
            {TRANSCRIPTION_ENGINES.map((en) => (
              <option key={en.id} value={en.id}>
                {en.label}
              </option>
            ))}
          </select>
        </label>
        <label className="text-xs text-gray-600 dark:text-gray-400">
          模型
          {usesGemini ? (
            <select
              value={geminiModel}
              onChange={(e) => setGeminiModel(e.target.value)}
              className="mt-1 block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
            >
              {GEMINI_TRANSCRIBE_MODELS.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label}
                </option>
              ))}
            </select>
          ) : (
            <select
              value={whisperModel}
              onChange={(e) => setWhisperModel(e.target.value)}
              className="mt-1 block w-full rounded-md border border-gray-300 bg-white p-2 text-sm dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
            >
              {WHISPER_MODELS.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label}
                </option>
              ))}
            </select>
          )}
        </label>
      </div>
      {usesGemini && (
        <p className="rounded-md bg-amber-50 p-2 text-xs text-amber-800 dark:bg-amber-900/30 dark:text-amber-300">
          ⚠️ Gemini 會把整段錄音上傳 Google 雲端,需在 Settings 設定 Gemini API key。
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={() => setOpen(false)}>
          取消
        </Button>
        <Button size="sm" onClick={() => void run()} disabled={busy}>
          {busy ? "啟動中…" : "開始重新轉錄"}
        </Button>
      </div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`-mb-px border-b-2 px-3 py-2 text-sm font-medium transition-colors ${
        active
          ? "border-blue-600 text-blue-600 dark:text-blue-400"
          : "border-transparent text-gray-500 hover:text-gray-800 dark:text-gray-400 dark:hover:text-gray-200"
      }`}
    >
      {children}
    </button>
  );
}

function Empty({
  message,
  onBack,
}: {
  message: string;
  onBack: () => void;
}) {
  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col items-start gap-3 p-6">
      <Button variant="ghost" size="sm" onClick={onBack}>
        ← History
      </Button>
      <p className="text-sm text-gray-500 dark:text-gray-400">{message}</p>
    </div>
  );
}
