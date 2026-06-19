import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, beforeEach } from "vitest";
import { MeetingList } from "@/components/Meeting/MeetingList";
import { useRecordingStore } from "@/stores/recordingStore";
import { VIEWS } from "@/lib/constants";

describe("MeetingList", () => {
  beforeEach(() => {
    useRecordingStore.setState({
      view: VIEWS.Meetings,
      selectedMeetingId: null,
    });
  });

  it("renders meetings from the mock backend", async () => {
    render(<MeetingList />);
    await waitFor(() =>
      expect(screen.getByText(/Q3 Planning/i)).toBeInTheDocument(),
    );
    expect(screen.getByText(/1:1 with Alex/i)).toBeInTheDocument();
  });

  it("opens a meeting detail when a row is clicked", async () => {
    const user = userEvent.setup();
    render(<MeetingList />);
    const row = await screen.findByText(/Q3 Planning/i);
    await user.click(row);
    const s = useRecordingStore.getState();
    expect(s.view).toBe(VIEWS.Detail);
    expect(s.selectedMeetingId).toBe("mtg-001");
  });

  it("filters via search", async () => {
    const user = userEvent.setup();
    render(<MeetingList />);
    await screen.findByText(/Q3 Planning/i);
    const input = screen.getByLabelText(/search transcripts/i);
    await user.type(input, "Alex");
    await user.click(screen.getByRole("button", { name: /^Search$/i }));
    await waitFor(() =>
      expect(screen.getByText(/1:1 with Alex/i)).toBeInTheDocument(),
    );
    expect(screen.queryByText(/Q3 Planning/i)).not.toBeInTheDocument();
  });
});
