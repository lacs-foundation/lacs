export type ShellMode =
  | "idle"
  | "planning"
  | "previewing"
  | "awaiting-approval"
  | "executing"
  | "needs-reboot"
  | "failed"
  | "rolled-back";

export type ShellOutcome = "succeeded" | "needs_reboot" | "failed" | "rolled_back";

export interface ShellPreview {
  summary: string;
}

export interface PlanStep {
  actionName: string;
  summary: string;
  riskLevel: string;
  approvalRequired: boolean;
  params: unknown;
}

export interface TimelineEntry {
  id: string;
  text: string;
}

// ---------------------------------------------------------------------------
// Discriminated union — invalid mode/field combinations are unrepresentable.
//
// Invariants encoded at the type level:
//   - "previewing" and "awaiting-approval" require a non-null preview
//   - "executing" requires a non-null activeJobId
//   - "idle" and "planning" have no preview and no activeJobId
// ---------------------------------------------------------------------------

type Base = { intent: string; timeline: TimelineEntry[] };

type IdleState        = Base & { mode: "idle";              preview: null;               activeJobId: null; steps: null    };
type PlanningState    = Base & { mode: "planning";          preview: null;               activeJobId: null; steps: null    };
type PreviewingState  = Base & { mode: "previewing";        preview: ShellPreview;       activeJobId: null; steps: PlanStep[] };
type ApprovingState   = Base & { mode: "awaiting-approval"; preview: ShellPreview;       activeJobId: null; steps: PlanStep[] };
type ExecutingState   = Base & { mode: "executing";         preview: ShellPreview;       activeJobId: string; steps: PlanStep[] };
type NeedsRebootState = Base & { mode: "needs-reboot";      preview: ShellPreview;       activeJobId: null; steps: PlanStep[] };
// Terminal error states may be reached before a preview exists (e.g. planning failure)
type FailedState      = Base & { mode: "failed";            preview: ShellPreview | null; activeJobId: null; steps: PlanStep[] | null };
type RolledBackState  = Base & { mode: "rolled-back";       preview: ShellPreview | null; activeJobId: null; steps: PlanStep[] | null };

export type ShellState =
  | IdleState
  | PlanningState
  | PreviewingState
  | ApprovingState
  | ExecutingState
  | NeedsRebootState
  | FailedState
  | RolledBackState;

export type ShellAction =
  | { type: "intent_submitted"; intent: string }
  | { type: "preview_ready"; summary: string; steps: PlanStep[] }
  | { type: "request_approval"; steps: PlanStep[] }
  | { type: "approval_granted" }
  | { type: "job_completed"; outcome: ShellOutcome }
  | { type: "timeline_event"; text: string }
  | { type: "reset" };

export const initialShellState: ShellState = {
  mode: "idle",
  intent: "",
  preview: null,
  activeJobId: null,
  steps: null,
  timeline: [],
};

export function shellReducer(state: ShellState, action: ShellAction): ShellState {
  switch (action.type) {
    case "intent_submitted": {
      const next: PlanningState = {
        mode: "planning",
        intent: action.intent,
        preview: null,
        activeJobId: null,
        steps: null,
        timeline: state.timeline,
      };
      return appendTimeline(next, `Intent submitted: ${action.intent}`);
    }

    case "preview_ready": {
      const next: PreviewingState = {
        mode: "previewing",
        intent: state.intent,
        preview: { summary: action.summary },
        activeJobId: null,
        steps: action.steps,
        timeline: state.timeline,
      };
      return appendTimeline(next, `Preview ready: ${action.summary}`);
    }

    case "request_approval": {
      // Only reachable from "previewing" which guarantees preview is non-null.
      const preview = state.preview as ShellPreview;
      const next: ApprovingState = {
        mode: "awaiting-approval",
        intent: state.intent,
        preview,
        activeJobId: null,
        steps: action.steps,
        timeline: state.timeline,
      };
      return appendTimeline(next, "Awaiting user approval");
    }

    case "approval_granted": {
      // Only reachable from "awaiting-approval" which guarantees preview is non-null.
      const preview = state.preview as ShellPreview;
      const steps = (state as ApprovingState).steps;
      const next: ExecutingState = {
        mode: "executing",
        intent: state.intent,
        preview,
        activeJobId: "pending",
        steps,
        timeline: state.timeline,
      };
      return appendTimeline(next, "Approval granted");
    }

    case "job_completed": {
      const steps = "steps" in state ? state.steps : null;
      if (action.outcome === "needs_reboot") {
        const next: NeedsRebootState = {
          mode: "needs-reboot",
          intent: state.intent,
          preview: state.preview as ShellPreview,
          activeJobId: null,
          steps: steps as PlanStep[],
          timeline: state.timeline,
        };
        return appendTimeline(next, "Job completed; reboot required");
      }
      if (action.outcome === "rolled_back") {
        const next: RolledBackState = {
          mode: "rolled-back",
          intent: state.intent,
          preview: state.preview,
          activeJobId: null,
          steps,
          timeline: state.timeline,
        };
        return appendTimeline(next, "Job rolled back");
      }
      if (action.outcome === "failed") {
        const next: FailedState = {
          mode: "failed",
          intent: state.intent,
          preview: state.preview,
          activeJobId: null,
          steps,
          timeline: state.timeline,
        };
        return appendTimeline(next, "Job failed");
      }
      // "succeeded"
      const next: IdleState = {
        mode: "idle",
        intent: state.intent,
        preview: null,
        activeJobId: null,
        steps: null,
        timeline: state.timeline,
      };
      return appendTimeline(next, "Job completed successfully");
    }

    case "timeline_event":
      return appendTimeline(state, action.text);

    case "reset":
      return initialShellState;

    default: {
      // TypeScript exhaustiveness check: if a new ShellAction variant is added
      // without a matching case, this line produces a compile error.
      const exhaustiveCheck: never = action;
      console.warn("[LACS] shellReducer received unknown action:", exhaustiveCheck);
      return state;
    }
  }
}

// Generic so the discriminant is preserved through the spread.
function appendTimeline<S extends ShellState>(state: S, text: string): S {
  return {
    ...state,
    timeline: [
      ...state.timeline,
      { id: String(state.timeline.length + 1), text },
    ],
  } as S;
}
