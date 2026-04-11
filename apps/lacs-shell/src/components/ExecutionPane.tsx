import { useState } from "react";
import type { ShellMode } from "../shellState";
import type { PlanResponse, ShellError } from "../types";
import { ErrorBlock } from "./ErrorBlock";

interface Props {
  mode: ShellMode;
  plan: PlanResponse | null;
  activeJobId: string | null;
  onCancel: () => void;
  onReset: () => void;
  executionError?: ShellError;
}

export function ExecutionPane({ mode, plan, activeJobId, onCancel, onReset, executionError }: Props) {
  const [isCanceling, setIsCanceling] = useState(false);

  const handleCancel = () => {
    setIsCanceling(true);
    onCancel();
  };

  if (mode === "failed" && executionError) {
    return (
      <section className="pane pane-execution">
        <h2>Execution</h2>
        <ErrorBlock error={executionError} onReset={onReset} />
        <button type="button" onClick={onReset}>New task</button>
      </section>
    );
  }

  if (mode === "needs-reboot" || mode === "rolled-back") {
    return (
      <section className="pane pane-execution">
        <h2>
          {mode === "needs-reboot" ? "Completed" : "Rolled back"}
        </h2>
        {mode === "needs-reboot" && (
          <div className="execution-reboot-banner" role="note">
            <p>⚠ Reboot required to apply changes.</p>
            <p>Run: <code>systemctl reboot</code></p>
          </div>
        )}
        <button type="button" onClick={onReset}>New task</button>
      </section>
    );
  }

  return (
    <section className="pane pane-execution">
      <div className="pane-header">
        <h2>Executing</h2>
        {activeJobId && <code className="execution-job-id">Job: {activeJobId}</code>}
      </div>

      {plan && (
        <ol className="execution-steps">
          {plan.steps.map((step, i) => (
            <li key={`${i}-${step.actionName}`} className="execution-step">
              <span className="execution-step__icon" aria-hidden>○</span>
              <span className="execution-step__index">{i + 1}/{plan.steps.length}</span>
              <code className="execution-step__name">{step.actionName}</code>
              <span className="execution-step__summary">{step.summary}</span>
            </li>
          ))}
        </ol>
      )}

      <div className="execution-actions">
        <button
          type="button"
          onClick={handleCancel}
          disabled={isCanceling}
        >
          {isCanceling ? "Canceling..." : "Cancel"}
        </button>
      </div>
    </section>
  );
}
