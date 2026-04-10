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

export interface TimelineEntry {
  id: string;
  text: string;
}

export interface ShellState {
  mode: ShellMode;
  intent: string;
  preview: ShellPreview | null;
  activeJobId: string | null;
  timeline: TimelineEntry[];
}

export type ShellAction =
  | { type: "intent_submitted"; intent: string }
  | { type: "preview_ready"; summary: string }
  | { type: "request_approval" }
  | { type: "approval_granted" }
  | { type: "job_completed"; outcome: ShellOutcome }
  | { type: "timeline_event"; text: string }
  | { type: "reset" };

export const initialShellState: ShellState = {
  mode: "idle",
  intent: "",
  preview: null,
  activeJobId: null,
  timeline: [],
};

export function shellReducer(state: ShellState, action: ShellAction): ShellState {
  switch (action.type) {
    case "intent_submitted":
      return appendTimeline({
        ...state,
        mode: "planning",
        intent: action.intent,
      }, `Intent submitted: ${action.intent}`);
    case "preview_ready":
      return appendTimeline({
        ...state,
        mode: "previewing",
        preview: { summary: action.summary },
      }, `Preview ready: ${action.summary}`);
    case "request_approval":
      return appendTimeline({
        ...state,
        mode: "awaiting-approval",
      }, "Awaiting user approval");
    case "approval_granted":
      return appendTimeline({
        ...state,
        mode: "executing",
        activeJobId: state.activeJobId ?? `job-${state.timeline.length + 1}`,
      }, "Approval granted");
    case "job_completed":
      if (action.outcome === "needs_reboot") {
        return appendTimeline({
          ...state,
          mode: "needs-reboot",
        }, "Job completed; reboot required");
      }
      if (action.outcome === "rolled_back") {
        return appendTimeline({
          ...state,
          mode: "rolled-back",
        }, "Job rolled back");
      }
      if (action.outcome === "failed") {
        return appendTimeline({
          ...state,
          mode: "failed",
        }, "Job failed");
      }
      return appendTimeline({
        ...state,
        mode: "idle",
        activeJobId: null,
      }, "Job completed successfully");
    case "timeline_event":
      return appendTimeline(state, action.text);
    case "reset":
      return initialShellState;
    default: {
      const exhaustiveCheck: never = action;
      console.warn("[LACS] shellReducer received unknown action:", exhaustiveCheck);
      return state;
    }
  }
}

function appendTimeline(state: ShellState, text: string): ShellState {
  return {
    ...state,
    timeline: [
      ...state.timeline,
      { id: String(state.timeline.length + 1), text },
    ],
  };
}
