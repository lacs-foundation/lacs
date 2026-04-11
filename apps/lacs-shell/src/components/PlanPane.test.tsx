import { render, screen, fireEvent } from "@testing-library/react";
import { PlanPane } from "./PlanPane";
import type { PlanResponse, ShellError } from "../types";

const LOW_PLAN: PlanResponse = {
  summary: "Read the system state",
  explanation: "Inspects the current deployment.",
  approvalRequired: false,
  steps: [
    { actionName: "GetSystemState", summary: "Read state", riskLevel: "low", approvalRequired: false, params: {} },
  ],
  hostName: "silverblue", deployment: "fedora/41", toolboxCount: 1, flatpakCount: 2,
};

const HIGH_PLAN: PlanResponse = {
  summary: "Install vim",
  explanation: "Layers vim via rpm-ostree.",
  approvalRequired: true,
  steps: [
    { actionName: "GetSystemState", summary: "Read state", riskLevel: "low", approvalRequired: false, params: {} },
    { actionName: "InstallPackages", summary: "Layer vim", riskLevel: "high", approvalRequired: true, params: {} },
  ],
  hostName: "silverblue", deployment: "fedora/41", toolboxCount: 0, flatpakCount: 0,
};

describe("PlanPane — plan display", () => {
  it("renders the plan summary and explanation", () => {
    render(<PlanPane plan={LOW_PLAN} mode="previewing" onApprove={() => {}} error={null} />);
    expect(screen.getByText("Read the system state")).toBeInTheDocument();
    expect(screen.getByText("Inspects the current deployment.")).toBeInTheDocument();
  });

  it("renders all step action names", () => {
    render(<PlanPane plan={HIGH_PLAN} mode="previewing" onApprove={() => {}} error={null} />);
    expect(screen.getByText("GetSystemState")).toBeInTheDocument();
    expect(screen.getByText("InstallPackages")).toBeInTheDocument();
  });

  it("renders the system context line", () => {
    render(<PlanPane plan={LOW_PLAN} mode="previewing" onApprove={() => {}} error={null} />);
    expect(screen.getByText(/silverblue/)).toBeInTheDocument();
    expect(screen.getByText(/fedora\/41/)).toBeInTheDocument();
  });

  it("renders an inline error when error prop is set", () => {
    const err: ShellError = { code: "role_insufficient", message: "Admin required", systemChanged: false };
    render(<PlanPane plan={LOW_PLAN} mode="previewing" onApprove={() => {}} error={err} />);
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });
});

describe("PlanPane — approval gate", () => {
  it("does not render the gate in previewing mode", () => {
    render(<PlanPane plan={LOW_PLAN} mode="previewing" onApprove={() => {}} error={null} />);
    expect(screen.queryByRole("button", { name: /approve/i })).toBeNull();
  });

  it("LOW risk: renders a single Approve button in awaiting-approval mode", () => {
    render(<PlanPane plan={LOW_PLAN} mode="awaiting-approval" onApprove={() => {}} error={null} />);
    expect(screen.getByRole("button", { name: /approve/i })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("HIGH risk: Approve button is disabled until action name is typed", () => {
    render(<PlanPane plan={HIGH_PLAN} mode="awaiting-approval" onApprove={() => {}} error={null} />);
    const approveBtn = screen.getByRole("button", { name: /approve/i });
    expect(approveBtn).toBeDisabled();

    fireEvent.change(screen.getByRole("textbox"), { target: { value: "InstallPackages" } });
    expect(approveBtn).not.toBeDisabled();
  });

  it("HIGH risk: Approve button calls onApprove when clicked", () => {
    const onApprove = vi.fn();
    render(<PlanPane plan={HIGH_PLAN} mode="awaiting-approval" onApprove={onApprove} error={null} />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "InstallPackages" } });
    fireEvent.click(screen.getByRole("button", { name: /approve/i }));
    expect(onApprove).toHaveBeenCalledOnce();
  });
});
