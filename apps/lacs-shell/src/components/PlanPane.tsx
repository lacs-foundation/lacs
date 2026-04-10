import type { ShellPreview } from "../shellState";

interface Props {
  preview: ShellPreview | null;
}

export function PlanPane({ preview }: Props) {
  return (
    <section className="pane">
      <h2>Plan</h2>
      <p className="pane-meta">{preview ? preview.summary : "No preview yet"}</p>
    </section>
  );
}
