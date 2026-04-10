import { useEffect, useReducer, useCallback, useState } from "react";
import { IntentPane } from "./components/IntentPane";
import { ExecutionPane } from "./components/ExecutionPane";
import { PlanPane } from "./components/PlanPane";
import { TimelinePane } from "./components/TimelinePane";
import {
  getBrainConfig,
  requestPlan,
  requestApproval,
  cancelJob,
  subscribeDaemonEvents,
} from "./daemonBridge";
import {
  initialShellState,
  shellReducer,
} from "./shellState";
import type { BrainConfigResponse, ShellError } from "./types";

const STATUS_LABELS: Record<string, string> = {
  idle:                "Ready",
  planning:            "Planning...",
  previewing:          "Review plan",
  "awaiting-approval": "Awaiting your approval",
  executing:           "Executing...",
  "needs-reboot":      "Done — reboot required",
  failed:              "Failed",
  "rolled-back":       "Rolled back",
};

export default function App() {
  const [state, dispatch] = useReducer(shellReducer, initialShellState);
  const [brainConfig, setBrainConfig] = useState<BrainConfigResponse | null>(null);

  // Load brain config once on mount
  useEffect(() => {
    getBrainConfig()
      .then((cfg) => {
        setBrainConfig(cfg);
        if (cfg.fallback) {
          dispatch({
            type: "timeline_event",
            text: `Brain config fallback: using ${cfg.provider}/${cfg.model}`,
            kind: "warning",
          });
        }
      })
      .catch(() => {
        // Non-fatal — UI degrades gracefully without the provider label
      });
  }, []);

  // Subscribe to daemon-pushed timeline and outcome events
  useEffect(() => {
    let unsubscribeFn: (() => void) | null = null;
    let cancelled = false;

    subscribeDaemonEvents(
      (payload) => {
        if (!cancelled) {
          dispatch({ type: "timeline_event", text: payload.text, kind: "system" });
        }
      },
      (outcome) => {
        if (!cancelled) {
          dispatch({ type: "job_completed", outcome });
        }
      },
    )
      .then((unsub) => {
        if (cancelled) unsub();
        else unsubscribeFn = unsub;
      })
      .catch(() => {
        if (!cancelled) {
          dispatch({ type: "daemon_status_changed", status: "unreachable" });
        }
      });

    return () => {
      cancelled = true;
      unsubscribeFn?.();
    };
  }, []);

  const handleIntent = useCallback(async (intent: string) => {
    if (!intent) return;
    dispatch({ type: "intent_submitted", intent });

    try {
      const plan = await requestPlan(intent);
      dispatch({ type: "daemon_status_changed", status: "connected" });
      dispatch({ type: "plan_ready", plan });
      if (plan.approvalRequired) {
        dispatch({ type: "request_approval" });
      } else {
        dispatch({ type: "timeline_event", text: `Read-only plan completed: ${intent}`, kind: "success" });
        dispatch({ type: "job_completed", outcome: "succeeded" });
      }
    } catch (err) {
      dispatch({ type: "daemon_status_changed", status: "unreachable" });
      const shellError: ShellError =
        err && typeof err === "object" && "code" in err
          ? (err as ShellError)
          : { code: "unknown", message: String(err), systemChanged: false };
      dispatch({ type: "plan_errored", error: shellError });
    }
  }, []);

  const handleApprove = useCallback(async () => {
    if (state.mode !== "awaiting-approval") return;
    const steps = state.plan.steps;
    // Dispatch approval_granted immediately so the UI transitions to "executing"
    // and stays responsive during the (potentially long) daemon execution.
    dispatch({ type: "approval_granted" });
    try {
      await requestApproval(steps);
      dispatch({ type: "daemon_status_changed", status: "connected" });
    } catch (_err) {
      // The state is now "executing" — policy_errored is only handled in
      // awaiting-approval/previewing mode and would be silently dropped here.
      // Use job_completed("failed") instead so the reducer transitions correctly.
      dispatch({ type: "daemon_status_changed", status: "unreachable" });
      dispatch({ type: "job_completed", outcome: "failed" });
    }
  }, [state]);

  const handleCancel = useCallback(async () => {
    dispatch({ type: "cancel_requested" });
    if (state.activeJobId) {
      try {
        await cancelJob(state.activeJobId);
      } catch {
        // Cancel failed — daemon will resolve the job eventually
      }
    }
  }, [state.activeJobId]);

  const handleReset = useCallback(() => {
    dispatch({ type: "reset" });
  }, []);

  const plan =
    state.mode === "previewing" ||
    state.mode === "awaiting-approval" ||
    state.mode === "executing" ||
    state.mode === "needs-reboot" ||
    state.mode === "rolled-back" ||
    state.mode === "failed"
      ? state.plan
      : null;

  const idleError = state.mode === "idle" ? state.error : null;
  const planError =
    state.mode === "previewing" || state.mode === "awaiting-approval"
      ? state.planError
      : null;

  const daemonLabel =
    state.daemonStatus === "connected" ? "daemon: connected" :
    state.daemonStatus === "unreachable" ? "daemon: unreachable" :
    "daemon: unknown";

  return (
    <main className="app-shell">
      <header className="app-header">
        <div>
          <p className="eyebrow">LACS</p>
          <h1>Linux Agent Control Standard</h1>
        </div>
        <div className="app-header__right">
          <div className="status-badge" role="status">
            {STATUS_LABELS[state.mode] ?? state.mode}
          </div>
          {brainConfig && (
            <p className="provider-label">
              {brainConfig.fallback && <span className="provider-label__fallback">⚠ </span>}
              via {brainConfig.provider}/{brainConfig.model}
            </p>
          )}
          <p className={`daemon-indicator daemon-indicator--${state.daemonStatus}`}>
            ● {daemonLabel}
          </p>
          {state.mode !== "executing" && (
            <button type="button" className="reset-btn" onClick={handleReset}>
              Reset
            </button>
          )}
        </div>
      </header>

      <section className="grid" data-mode={state.mode}>
        <IntentPane
          intent={state.intent}
          mode={state.mode}
          onSubmit={handleIntent}
          onReset={handleReset}
          error={idleError}
        />
        {(state.mode === "previewing" || state.mode === "awaiting-approval") && plan && (
          <PlanPane
            plan={plan}
            mode={state.mode}
            onApprove={handleApprove}
            error={planError ?? null}
          />
        )}
        {(state.mode === "executing" ||
          state.mode === "needs-reboot" ||
          state.mode === "rolled-back" ||
          state.mode === "failed") && (
          <ExecutionPane
            mode={state.mode}
            plan={plan}
            activeJobId={state.activeJobId}
            onCancel={handleCancel}
            onReset={handleReset}
          />
        )}
        <TimelinePane entries={state.timeline} />
      </section>
    </main>
  );
}
