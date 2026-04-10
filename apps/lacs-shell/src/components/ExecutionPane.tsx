import type { ShellMode } from "../shellState";

interface Props {
  mode: ShellMode;
  activeJobId: string | null;
}

export function ExecutionPane({ mode, activeJobId }: Props) {
  return (
    <section className="pane">
      <h2>Execution</h2>
      <p className="pane-meta">State: {mode}</p>
      <p className="pane-meta">
        Active job: {activeJobId ? activeJobId : "none"}
      </p>
    </section>
  );
}
