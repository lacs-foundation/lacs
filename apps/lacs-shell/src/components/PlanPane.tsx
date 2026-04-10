import type { PlanStep, ShellMode, ShellPreview } from "../shellState";

interface Props {
  preview: ShellPreview | null;
  steps: PlanStep[] | null;
  mode: ShellMode;
  onApprove?: () => void;
}

export function PlanPane({ preview, steps, mode, onApprove }: Props) {
  return (
    <section className="pane">
      <h2>Plan</h2>
      <p className="pane-meta">{preview ? preview.summary : "No preview yet"}</p>

      {steps && steps.length > 0 && (
        <ol className="plan-steps">
          {steps.map((step, i) => (
            <li key={i} className={`plan-step risk-${step.riskLevel}`}>
              <span className="step-summary">{step.summary}</span>
              <span className="step-risk">{step.riskLevel}</span>
            </li>
          ))}
        </ol>
      )}

      {mode === "awaiting-approval" && onApprove && (
        <button className="approve-btn" onClick={onApprove}>
          Approve &amp; Execute
        </button>
      )}
    </section>
  );
}
