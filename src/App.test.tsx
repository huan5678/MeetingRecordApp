import { render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import App from "./App";

describe("App shell", () => {
  it("renders the app wordmark", async () => {
    render(<App />);
    // The wordmark is split across spans ("Meeting" + "Record"); match the
    // combined text on the home button.
    expect(
      screen.getByRole("button", { name: /meeting\s*record/i }),
    ).toBeInTheDocument();
    // Let the async mock meeting load settle so React state updates are flushed.
    await waitFor(() =>
      expect(screen.getByText(/Q3 Planning/i)).toBeInTheDocument(),
    );
  });

  it("renders the local-first tagline", async () => {
    render(<App />);
    expect(screen.getByText(/local-first recorder/i)).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByText(/Q3 Planning/i)).toBeInTheDocument(),
    );
  });
});
