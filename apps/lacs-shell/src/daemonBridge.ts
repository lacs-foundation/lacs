import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ShellOutcome, ShellPreview, TimelineEntry } from "./shellState";

export interface DaemonPlanResponse {
  summary: string;
  preview: ShellPreview;
  approvalRequired: boolean;
}

export async function requestPlan(intent: string): Promise<DaemonPlanResponse> {
  if (!isTauriRuntime()) {
    return {
      summary: `Demo plan for ${intent}`,
      preview: { summary: `Demo preview for ${intent}` },
    };
  }

  return invoke<DaemonPlanResponse>("plan_intent", { intent });
}

export async function requestApproval(requestHash: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke("approve_preview", { requestHash });
}

export async function subscribeDaemonEvents(
  onPreview: (payload: ShellPreview) => void,
  onTimeline: (payload: TimelineEntry) => void,
  onOutcome: (payload: ShellOutcome) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }

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
