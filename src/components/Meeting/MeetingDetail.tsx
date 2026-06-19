/**
 * MeetingDetail — the single-meeting view (PRD §3.5 #28): player + transcript +
 * summary together, with tabs to switch focus, plus Export and Back actions.
 */

import { useState } from "react";
import { useMeetingDetail } from "@/hooks/useMeetings";
import { useTranscription } from "@/hooks/useTranscription";
import { useRecordingStore } from "@/stores/recordingStore";
import { MeetingPlayer } from "@/components/Meeting/MeetingPlayer";
import { Transcript } from "@/components/Meeting/Transcript";
import { SummaryView } from "@/components/Summary/SummaryView";
import { ExportDialog } from "@/components/Export/ExportDialog";
import { Button } from "@/components/common/Button";
import { formatDateTime, formatDuration } from "@/lib/format";
import {
  MEETING_STATUS,
  MEETING_TYPE_LABELS,
  VIEWS,
} from "@/lib/constants";

type Tab = "summary" | "transcript";

export function MeetingDetail() {
  const meetingId = useRecordingStore((s) => s.selectedMeetingId);
  const navigate = useRecordingStore((s) => s.navigate);
  const { detail, loading, error, reload } = useMeetingDetail(meetingId);
  const transcription = useTranscription(meetingId, !!detail);
  const [tab, setTab] = useState<Tab>("summary");
  const [exporting, setExporting] = useState(false);

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

  const { meeting, media, segments, summary } = detail;
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
          <h1 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
            {meeting.title ?? "Untitled meeting"}
          </h1>
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
          <SummaryView
            meetingId={meeting.id}
            summary={summary}
            onRegenerated={() => void reload()}
          />
        ) : (
          <Transcript segments={segments} readOnly={stillTranscribing} />
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
