import {
  initialShellState,
  shellReducer,
  type ShellMode,
} from "./shellState";

describe("shellReducer", () => {
  it("starts in idle mode", () => {
    expect(initialShellState.mode).toBe("idle");
  });

  it("moves from idle to planning to previewing", () => {
    const planning = shellReducer(initialShellState, {
      type: "intent_submitted",
      intent: "update this machine",
    });
    const previewing = shellReducer(planning, {
      type: "preview_ready",
      summary: "UpdateSystem preview",
    });

    expect(planning.mode).toBe("planning");
    expect(previewing.mode).toBe("previewing");
    expect(previewing.preview?.summary).toBe("UpdateSystem preview");
  });

  it("requires approval before execution and can move to reboot or rollback states", () => {
    const previewing = {
      ...initialShellState,
      mode: "previewing" as ShellMode,
      preview: { summary: "UpdateSystem preview" },
    };
    const awaitingApproval = shellReducer(previewing, { type: "request_approval" });
    const executing = shellReducer(awaitingApproval, { type: "approval_granted" });
    const needsReboot = shellReducer(executing, { type: "job_completed", outcome: "needs_reboot" });

    expect(awaitingApproval.mode).toBe("awaiting-approval");
    expect(executing.mode).toBe("executing");
    expect(needsReboot.mode).toBe("needs-reboot");
  });
});
