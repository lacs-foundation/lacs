import type { FormEvent } from "react";
import type { ShellMode } from "../shellState";

interface Props {
  intent: string;
  mode: ShellMode;
  onSubmit: (intent: string) => void;
}

export function IntentPane({ intent, mode, onSubmit }: Props) {
  return (
    <section className="pane">
      <h2>Intent</h2>
      <p className="pane-meta">Mode: {mode}</p>
      <form
        className="intent-form"
        onSubmit={(event: FormEvent<HTMLFormElement>) => {
          event.preventDefault();
          const formData = new FormData(event.currentTarget);
          onSubmit(String(formData.get("intent") ?? "").trim());
        }}
      >
        <label className="field">
          <span>What should LACS do?</span>
          <input name="intent" defaultValue={intent} placeholder="update this machine" />
        </label>
        <button type="submit">Generate plan</button>
      </form>
    </section>
  );
}
