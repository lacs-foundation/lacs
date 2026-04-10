import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import App from "./App";
import * as bridge from "./daemonBridge";
import type { PlanResponse } from "./types";

vi.mock("./daemonBridge", () => ({
  requestPlan: vi.fn(),
  requestApproval: vi.fn().mockResolvedValue(undefined),
  cancelJob: vi.fn().mockResolvedValue(undefined),
  getBrainConfig: vi.fn().mockResolvedValue({
    provider: "ollama",
    model: "mistral:7b",
    fallback: false,
  }),
  subscribeDaemonEvents: vi.fn().mockResolvedValue(() => undefined),
}));

const mockedRequestPlan = vi.mocked(bridge.requestPlan);

const READ_ONLY_PLAN: PlanResponse = {
  summary: "Inspect system state",
  explanation: "Reads the current deployment.",
  approvalRequired: false,
  steps: [
    { actionName: "GetSystemState", summary: "Read state", riskLevel: "low", approvalRequired: false, params: {} },
  ],
  hostName: "silverblue", deployment: "fedora/41", toolboxCount: 1, flatpakCount: 2,
};

const MUTATING_PLAN: PlanResponse = {
  summary: "Install vim",
  explanation: "Layers vim via rpm-ostree.",
  approvalRequired: true,
  steps: [
    { actionName: "InstallPackages", summary: "Layer vim", riskLevel: "high", approvalRequired: true, params: {} },
  ],
  hostName: "silverblue", deployment: "fedora/41", toolboxCount: 0, flatpakCount: 0,
};

describe("App", () => {
  it("renders the shell and shows Ready status", () => {
    render(<App />);
    expect(screen.getByRole("status")).toHaveTextContent("Ready");
  });

  it("shows 'Review plan' status for read-only plan (no approval required)", async () => {
    mockedRequestPlan.mockResolvedValueOnce(READ_ONLY_PLAN);
    render(<App />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "show state" } });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Ready");
    });
  });

  it("transitions to 'Awaiting your approval' for mutating plans", async () => {
    mockedRequestPlan.mockResolvedValueOnce(MUTATING_PLAN);
    render(<App />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "install vim" } });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Awaiting your approval");
    });
  });

  it("stays on Ready and shows error when requestPlan rejects", async () => {
    mockedRequestPlan.mockRejectedValueOnce({
      code: "llm_http_error",
      message: "HTTP 500",
      systemChanged: false,
    });
    render(<App />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "install vim" } });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Ready");
    });
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });

  it("sets grid data-mode attribute to the current mode", async () => {
    mockedRequestPlan.mockResolvedValueOnce(MUTATING_PLAN);
    render(<App />);
    expect(document.querySelector(".grid")?.getAttribute("data-mode")).toBe("idle");

    fireEvent.change(screen.getByRole("textbox"), { target: { value: "install vim" } });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));

    await waitFor(() => {
      expect(document.querySelector(".grid")?.getAttribute("data-mode")).toBe("awaiting-approval");
    });
  });
});
