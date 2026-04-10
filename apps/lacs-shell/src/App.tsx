import { useEffect, useReducer } from "react";
import { IntentPane } from "./components/IntentPane";
import { ExecutionPane } from "./components/ExecutionPane";
import { PlanPane } from "./components/PlanPane";
import { TimelinePane } from "./components/TimelinePane";
import { requestPlan, subscribeDaemonEvents } from "./daemonBridge";
import {
  initialShellState,
  shellReducer,
} from "./shellState";

export default function App() {
  const [state, dispatch] = useReducer(shellReducer, initialShellState);

  useEffect(() => {
    let alive = true;
    void subscribeDaemonEvents(
      (payload) => {
        if (alive) {
          dispatch({ type: "preview_ready", summary: payload.summary });
        }
      },
      (payload) => {
        if (alive) {
          dispatch({ type: "timeline_event", text: payload.text });
        }
      },
      (outcome) => {
        if (alive) {
          dispatch({ type: "job_completed", outcome });
        }
      },
    ).then((unsubscribe) => {
      if (!alive) {
        unsubscribe();
      }
    });

    return () => {
      alive = false;
    };
  }, []);

  async function handleIntent(intent: string) {
    if (!intent) {
      return;
    }

    dispatch({ type: "intent_submitted", intent });
    const response = await requestPlan(intent);
    dispatch({ type: "preview_ready", summary: response.preview.summary });
    if (response.approvalRequired) {
      dispatch({ type: "request_approval" });
    } else {
      dispatch({
        type: "timeline_event",
        text: `Read-only intent completed: ${intent}`,
      });
      dispatch({ type: "job_completed", outcome: "succeeded" });
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
        <PlanPane preview={state.preview} />
        <ExecutionPane mode={state.mode} activeJobId={state.activeJobId} />
        <TimelinePane entries={state.timeline} />
      </section>
    </main>
  );
}
