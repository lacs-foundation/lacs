import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ShellOutcome, ShellPreview, TimelineEntry } from "./shellState";

export interface DaemonPlanResponse {
  summary: string;
  preview: ShellPreview;
  approvalRequired: boolean;
}

function requireTauriRuntime(): void {
  if (!isTauriRuntime()) {
    throw new Error(
      "LACS Shell is not running inside a Tauri runtime. The daemon bridge is unavailable.",
    );
  }
}

export async function requestPlan(intent: string): Promise<DaemonPlanResponse> {
  requireTauriRuntime();
  return invoke<DaemonPlanResponse>("plan_intent", { intent });
}

export async function requestApproval(requestHash: string): Promise<void> {
  requireTauriRuntime();
  await invoke("approve_preview", { requestHash });
}

export async function subscribeDaemonEvents(
  onPreview: (payload: ShellPreview) => void,
  onTimeline: (payload: TimelineEntry) => void,
  onOutcome: (payload: ShellOutcome) => void,
): Promise<() => void> {
  requireTauriRuntime();

  const previewUnlisten = await listen<ShellPreview>("lacs:preview-ready", (event) => {
    onPreview(event.payload);
  });
  const timelineUnlisten = await listen<TimelineEntry>("lacs:timeline-entry", (event) => {
    onTimeline(event.payload);
  });
  const outcomeUnlisten = await listen<ShellOutcome>("lacs:job-completed", (event) => {
    onOutcome(event.payload);
  });

  return () => {
    previewUnlisten();
    timelineUnlisten();
    outcomeUnlisten();
  };
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window;
}
