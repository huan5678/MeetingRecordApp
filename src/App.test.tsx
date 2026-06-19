import { render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import App from "./App";

describe("App shell", () => {
  it("renders the app title", async () => {
    render(<App />);
    expect(
      screen.getByRole("heading", { name: /MeetingRecordApp/i }),
    ).toBeInTheDocument();
    // Let the async mock meeting load settle so React state updates are flushed.
    await waitFor(() =>
      expect(screen.getByText(/Q3 Planning/i)).toBeInTheDocument(),
    );
  });

  it("renders the v1.0 scope tagline", async () => {
    render(<App />);
    expect(
      screen.getByText(/Windows, audio-only, local-first/i),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByText(/Q3 Planning/i)).toBeInTheDocument(),
    );
  });
});
