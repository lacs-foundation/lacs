import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { BrainConfigResponse, DaemonStatus, PlanResponse, PlanStepResponse, ShellError } from "./types";
import type { ShellOutcome, TimelineEntry, TimelineEntryKind } from "./shellState";

// ---------------------------------------------------------------------------
// Bridge functions
// ---------------------------------------------------------------------------

export async function requestPlan(intent: string): Promise<PlanResponse> {
  requireTauriRuntime();
  return invoke<PlanResponse>("plan_intent", { intent });
}

/** Pass approved steps (with params) to the daemon for execution. */
export async function requestApproval(steps: PlanStepResponse[]): Promise<void> {
  requireTauriRuntime();
  // approve_preview expects { steps: [{ actionName, params }] }
  await invoke("approve_preview", {
    steps: steps.map((s) => ({ actionName: s.actionName, params: s.params })),
  });
}

export async function cancelJob(jobId: string): Promise<void> {
  requireTauriRuntime();
  await invoke("cancel_job", { jobId });
}

export async function getBrainConfig(): Promise<BrainConfigResponse> {
  requireTauriRuntime();
  return invoke<BrainConfigResponse>("get_brain_config");
}

export async function subscribeDaemonEvents(
  onTimeline: (payload: TimelineEntry) => void,
  onOutcome: (payload: ShellOutcome) => void,
): Promise<() => void> {
  requireTauriRuntime();

  const timelineUnlisten = await listen<{ id: string; text: string }>(
    "lacs:timeline-entry",
    (event) => {
      onTimeline({
        id: event.payload.id,
        timestamp: new Date().toLocaleTimeString("en-US", {
          hour12: false,
          hour: "2-digit",
          minute: "2-digit",
          second: "2-digit",
        }),
        kind: "system" as TimelineEntryKind,
        text: event.payload.text,
      });
    },
  );

  let outcomeUnlisten: (() => void) | null = null;
  try {
    outcomeUnlisten = await listen<ShellOutcome>("lacs:job-completed", (event) => {
      onOutcome(event.payload);
    });
  } catch (err) {
    timelineUnlisten();
    throw err;
  }

  const captured = outcomeUnlisten;
  return () => {
    timelineUnlisten();
    captured();
  };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function requireTauriRuntime(): void {
  if (!isTauriRuntime()) {
    throw new Error(
      "LACS Shell is not running inside a Tauri runtime. The daemon bridge is unavailable.",
    );
  }
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window;
}

export async function withDaemonStatus<T>(
  fn: () => Promise<T>,
): Promise<{ result: T; status: DaemonStatus }> {
  try {
    const result = await fn();
    return { result, status: "connected" };
  } catch {
    return { result: undefined as never, status: "unreachable" };
  }
}

// Re-export ShellError for callers that want it from one place
export type { ShellError };
