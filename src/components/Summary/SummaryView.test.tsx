import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { SummaryView } from "@/components/Summary/SummaryView";
import type { Summary } from "@/lib/types";

const SUMMARY: Summary = {
  id: "s1",
  meeting_id: "m1",
  summary_type: "auto",
  content: "## Overview\n\nThe team aligned on three themes.",
  action_items: [
    { task: "Do the spike", owner: "Alex", deadline: "2026-06-25", done: false },
  ],
  key_decisions: [{ decision: "De-risk audio first", context: "top risk" }],
  prompt_used: null,
  ai_provider: "ollama",
  ai_model: "llama3.1",
  tokens_used: 4231,
  created_at: "2026-06-18T15:31:00Z",
};

describe("SummaryView", () => {
  it("renders the markdown body, decisions, and action items", () => {
    render(<SummaryView meetingId="m1" summary={SUMMARY} />);
    expect(
      screen.getByRole("heading", { name: /Overview/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/De-risk audio first/i)).toBeInTheDocument();
    expect(screen.getByText(/Do the spike/i)).toBeInTheDocument();
    expect(screen.getByText(/via ollama/i)).toBeInTheDocument();
  });

  it("shows a generate CTA when there is no summary", () => {
    render(<SummaryView meetingId="m1" summary={null} />);
    expect(
      screen.getByRole("button", { name: /Generate summary/i }),
    ).toBeInTheDocument();
  });
});
