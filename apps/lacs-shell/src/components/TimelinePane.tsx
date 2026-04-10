import type { TimelineEntry } from "../shellState";

interface Props {
  entries: TimelineEntry[];
}

export function TimelinePane({ entries }: Props) {
  return (
    <section className="pane">
      <h2>Timeline</h2>
      <ol className="timeline">
        {entries.length === 0 ? <li>No events yet</li> : null}
        {entries.map((entry) => (
          <li key={entry.id}>{entry.text}</li>
        ))}
      </ol>
    </section>
  );
}
