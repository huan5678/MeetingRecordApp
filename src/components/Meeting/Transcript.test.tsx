import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { Transcript } from "@/components/Meeting/Transcript";
import { api } from "@/lib/tauri";
import type { TranscriptSegment } from "@/lib/types";

function seg(id: string, speaker: string | null, text: string): TranscriptSegment {
  return {
    id,
    meeting_id: "m1",
    segment_index: 0,
    start_time_ms: 0,
    end_time_ms: 900,
    text,
    speaker,
    confidence: null,
    language: null,
    created_at: "",
  };
}

describe("Transcript speaker labels", () => {
  const segs = [
    seg("s1", "Speaker 1", "hello"),
    seg("s2", "Speaker 2", "hi there"),
    seg("s3", "Speaker 1", "again"),
  ];

  it("resolves the display name for every segment sharing a raw label", () => {
    render(<Transcript segments={segs} labels={{ "Speaker 1": "Alice" }} meetingId="m1" />);
    // Both "Speaker 1" rows show "Alice"; "Speaker 2" is untouched.
    expect(screen.getAllByText("Alice")).toHaveLength(2);
    expect(screen.getByText("Speaker 2")).toBeInTheDocument();
    expect(screen.queryByText("Speaker 1")).not.toBeInTheDocument();
  });

  it("renames a speaker by its raw label and asks the parent to refresh", async () => {
    const user = userEvent.setup();
    const onRelabel = vi.fn();
    const spy = vi.spyOn(api, "setSpeakerLabel").mockResolvedValue();

    // Already labelled "Alice"; renaming must still target the RAW "Speaker 1".
    render(
      <Transcript
        segments={segs}
        labels={{ "Speaker 1": "Alice" }}
        meetingId="m1"
        onRelabel={onRelabel}
      />,
    );

    await user.click(screen.getAllByRole("button", { name: /rename speaker/i })[0]);
    const input = screen.getByRole("textbox", { name: /speaker name/i });
    await user.clear(input);
    await user.type(input, "Alicia{Enter}");

    await waitFor(() =>
      expect(spy).toHaveBeenCalledWith("m1", "Speaker 1", "Alicia"),
    );
    await waitFor(() => expect(onRelabel).toHaveBeenCalled());
    spy.mockRestore();
  });
});
