import { useEffect, useReducer } from "react";
import { IntentPane } from "./components/IntentPane";
import { ExecutionPane } from "./components/ExecutionPane";
import { PlanPane } from "./components/PlanPane";
import { TimelinePane } from "./components/TimelinePane";
import { requestApproval, requestPlan, subscribeDaemonEvents } from "./daemonBridge";
import {
  initialShellState,
  shellReducer,
} from "./shellState";

export default function App() {
  const [state, dispatch] = useReducer(shellReducer, initialShellState);

  useEffect(() => {
    let unsubscribeFn: (() => void) | null = null;
    let cancelled = false;

    subscribeDaemonEvents(
      (payload) => {
        if (!cancelled) {
          dispatch({ type: "preview_ready", summary: payload.summary });
        }
      },
      (payload) => {
        if (!cancelled) {
          dispatch({ type: "timeline_event", text: payload.text });
        }
      },
      (outcome) => {
        if (!cancelled) {
          dispatch({ type: "job_completed", outcome });
        }
      },
    )
      .then((unsub) => {
        if (cancelled) {
          unsub();
        } else {
          unsubscribeFn = unsub;
        }
      })
      .catch((err) => {
        console.error("[LACS] Failed to subscribe to daemon events:", err);
        if (!cancelled) {
          dispatch({ type: "job_completed", outcome: "failed" });
        }
      });

    return () => {
      cancelled = true;
      unsubscribeFn?.();
    };
  }, []);

  async function handleIntent(intent: string) {
    if (!intent) {
      return;
    }

    dispatch({ type: "intent_submitted", intent });
    try {
      const response = await requestPlan(intent);
      dispatch({ type: "preview_ready", summary: response.preview.summary, steps: response.steps });
      if (response.approvalRequired) {
        dispatch({ type: "request_approval", steps: response.steps });
      } else {
        dispatch({
          type: "timeline_event",
          text: `Read-only intent completed: ${intent}`,
        });
        dispatch({ type: "job_completed", outcome: "succeeded" });
      }
    } catch (err) {
      console.error("[LACS] requestPlan failed:", err);
      dispatch({ type: "timeline_event", text: `Planning failed: ${String(err)}` });
      dispatch({ type: "job_completed", outcome: "failed" });
    }
  }

  async function handleApprove() {
    if (state.mode !== "awaiting-approval") return;
    dispatch({ type: "approval_granted" });
    try {
      await requestApproval(state.steps);
    } catch (err) {
      console.error("[LACS] requestApproval failed:", err);
      dispatch({ type: "timeline_event", text: `Approval failed: ${String(err)}` });
      dispatch({ type: "job_completed", outcome: "failed" });
    }
  }

  return (
    <main className="app-shell">
      <header className="app-header">
        <div>
          <p className="eyebrow">LACS</p>
          <h1>Linux Agent Control Standard</h1>
        </div>
        <div className="status-badge" role="status">
          {state.mode}
        </div>
      </header>

      <section className="grid">
        <IntentPane intent={state.intent} mode={state.mode} onSubmit={handleIntent} />
        <PlanPane
          preview={state.preview}
          steps={"steps" in state ? state.steps : null}
          mode={state.mode}
          onApprove={state.mode === "awaiting-approval" ? handleApprove : undefined}
        />
        <ExecutionPane mode={state.mode} activeJobId={state.activeJobId} />
        <TimelinePane entries={state.timeline} />
      </section>
    </main>
  );
}
