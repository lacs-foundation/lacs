import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import App from "./App";
import { requestPlan } from "./daemonBridge";

vi.mock("./daemonBridge", () => ({
  requestPlan: vi.fn(),
  requestApproval: vi.fn(),
  subscribeDaemonEvents: vi.fn().mockResolvedValue(() => undefined),
}));

const mockedRequestPlan = vi.mocked(requestPlan);

describe("App", () => {
  it("renders the four-pane control surface", () => {
    render(<App />);

    expect(screen.getByText("Intent")).toBeInTheDocument();
    expect(screen.getByText("Plan")).toBeInTheDocument();
    expect(screen.getByText("Execution")).toBeInTheDocument();
    expect(screen.getByText("Timeline")).toBeInTheDocument();
  });

  it("shows the idle shell mode on first render", () => {
    render(<App />);

    expect(screen.getByRole("status")).toHaveTextContent("idle");
  });

  it("completes read-only intents without awaiting approval", async () => {
    mockedRequestPlan.mockResolvedValueOnce({
      summary: "Read-only inspection",
      preview: { summary: "Preview for show me the machine state" },
      approvalRequired: false,
    });

    render(<App />);

    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "show me the machine state" },
    });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));

    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("idle");
    });
    expect(
      screen.getByText("Read-only intent completed: show me the machine state"),
    ).toBeInTheDocument();
  });

  it("transitions to awaiting-approval for mutating intents", async () => {
    mockedRequestPlan.mockResolvedValueOnce({
      summary: "Update plan",
      preview: { summary: "Preview for update this machine" },
      approvalRequired: true,
    });

    render(<App />);

    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "update this machine" },
    });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));

    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("awaiting-approval");
    });
  });

  it("transitions to failed and surfaces the error when requestPlan rejects", async () => {
    mockedRequestPlan.mockRejectedValueOnce(new Error("daemon unavailable"));

    render(<App />);

    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "update this machine" },
    });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));

    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("failed");
    });
    expect(screen.getByText(/Planning failed:/)).toBeInTheDocument();
  });
});
