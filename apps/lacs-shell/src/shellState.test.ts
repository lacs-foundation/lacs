import {
  initialShellState,
  shellReducer,
  type PlanStep,
} from "./shellState";

const stubStep: PlanStep = {
  actionName: "UpdateSystem",
  summary: "Update all system packages",
  riskLevel: "high",
  approvalRequired: true,
  params: {},
};

// Helper: drive through intent_submitted → preview_ready → request_approval
function reachAwaitingApproval() {
  return shellReducer(
    shellReducer(
      shellReducer(initialShellState, {
        type: "intent_submitted",
        intent: "update this machine",
      }),
      { type: "preview_ready", summary: "UpdateSystem preview", steps: [stubStep] },
    ),
    { type: "request_approval", steps: [stubStep] },
  );
}

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
      steps: [stubStep],
    });

    expect(planning.mode).toBe("planning");
    expect(previewing.mode).toBe("previewing");
    expect(previewing.preview?.summary).toBe("UpdateSystem preview");
  });

  it("requires approval before execution and transitions to needs-reboot", () => {
    const awaitingApproval = reachAwaitingApproval();
    const executing = shellReducer(awaitingApproval, { type: "approval_granted" });
    const needsReboot = shellReducer(executing, {
      type: "job_completed",
      outcome: "needs_reboot",
    });

    expect(awaitingApproval.mode).toBe("awaiting-approval");
    expect(executing.mode).toBe("executing");
    expect(executing.activeJobId).not.toBeNull();
    expect(needsReboot.mode).toBe("needs-reboot");
  });

  it("approval_granted transitions to executing with a non-null activeJobId", () => {
    const executing = shellReducer(reachAwaitingApproval(), {
      type: "approval_granted",
    });

    expect(typeof executing.activeJobId).toBe("string");
    expect((executing.activeJobId as string).length).toBeGreaterThan(0);
  });

  it("handles job_completed with failed outcome", () => {
    // Can be reached from planning state (e.g. handleIntent catches an error)
    const planning = shellReducer(initialShellState, {
      type: "intent_submitted",
      intent: "update this machine",
    });
    const failed = shellReducer(planning, {
      type: "job_completed",
      outcome: "failed",
    });

    expect(failed.mode).toBe("failed");
    expect(failed.activeJobId).toBeNull();
  });

  it("handles job_completed with rolled_back outcome", () => {
    const executing = shellReducer(reachAwaitingApproval(), {
      type: "approval_granted",
    });
    const rolledBack = shellReducer(executing, {
      type: "job_completed",
      outcome: "rolled_back",
    });

    expect(rolledBack.mode).toBe("rolled-back");
    expect(rolledBack.activeJobId).toBeNull();
  });

  it("handles job_completed with succeeded outcome", () => {
    const executing = shellReducer(reachAwaitingApproval(), {
      type: "approval_granted",
    });
    const succeeded = shellReducer(executing, {
      type: "job_completed",
      outcome: "succeeded",
    });

    expect(succeeded.mode).toBe("idle");
    expect(succeeded.activeJobId).toBeNull();
    expect(succeeded.preview).toBeNull();
  });

  it("reset returns to idle from failed state", () => {
    const failed = shellReducer(
      shellReducer(initialShellState, {
        type: "intent_submitted",
        intent: "update",
      }),
      { type: "job_completed", outcome: "failed" },
    );
    const afterReset = shellReducer(failed, { type: "reset" });

    expect(afterReset.mode).toBe("idle");
    expect(afterReset.activeJobId).toBeNull();
    expect(afterReset.preview).toBeNull();
    expect(afterReset.intent).toBe("");
  });

  it("reset returns to idle from executing state", () => {
    const executing = shellReducer(reachAwaitingApproval(), {
      type: "approval_granted",
    });
    const afterReset = shellReducer(executing, { type: "reset" });

    expect(afterReset.mode).toBe("idle");
  });
});
