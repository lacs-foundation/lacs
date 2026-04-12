import { useState } from "react";

type WizardStep = "select" | "configure" | "done";
type Provider = "ollama" | "anthropic";

interface Props {
  onDismiss: () => void;
}

const CONFIG_PATH = "~/.config/lacs/config.toml";

function ollamaConfig(): string {
  return `[llm]
provider = "ollama"
model    = "qwen3:8b"`;
}

function anthropicConfig(): string {
  return `[llm]
provider = "anthropic"
model    = "claude-sonnet-4-20250514"`;
}

export function SetupWizard({ onDismiss }: Props) {
  const [step, setStep] = useState<WizardStep>("select");
  const [provider, setProvider] = useState<Provider | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);

  const handleSelectProvider = (p: Provider) => {
    setProvider(p);
    if (p === "ollama") {
      setStep("configure");
    }
    // For anthropic, stay on "select" to collect the API key, then
    // user clicks Continue to go to "configure".
  };

  const handleContinueToConfig = () => {
    setStep("configure");
  };

  const handleContinueToDone = () => {
    setStep("done");
  };

  const handleCopy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopyFailed(true);
      setTimeout(() => setCopyFailed(false), 2000);
    }
  };

  const configContent = provider === "anthropic" ? anthropicConfig() : ollamaConfig();

  // ----- Provider selection -----
  if (step === "select" && provider !== "anthropic") {
    return (
      <section className="pane setup-wizard">
        <h2>Choose your LLM provider</h2>
        <p className="setup-wizard__subtitle">
          LACS needs an LLM to generate administration plans. Pick one to get started.
        </p>
        <div className="setup-wizard__cards">
          <button
            type="button"
            className="setup-wizard__card"
            onClick={() => handleSelectProvider("ollama")}
          >
            <span className="setup-wizard__card-title">Ollama</span>
            <span className="setup-wizard__card-tag">recommended</span>
            <p className="setup-wizard__card-desc">
              Local inference, no API key required. Runs entirely on your machine.
            </p>
          </button>
          <button
            type="button"
            className="setup-wizard__card"
            onClick={() => handleSelectProvider("anthropic")}
          >
            <span className="setup-wizard__card-title">Anthropic</span>
            <span className="setup-wizard__card-tag">cloud</span>
            <p className="setup-wizard__card-desc">
              Requires an API key. Higher quality, needs internet access.
            </p>
          </button>
        </div>
        <button type="button" className="setup-wizard__skip" onClick={onDismiss}>
          Skip setup
        </button>
      </section>
    );
  }

  // ----- Anthropic API key input -----
  if (step === "select" && provider === "anthropic") {
    return (
      <section className="pane setup-wizard">
        <h2>Anthropic API key</h2>
        <p className="setup-wizard__subtitle">
          Enter your Anthropic API key. You can find it at{" "}
          <code>console.anthropic.com</code>.
        </p>
        <div className="setup-wizard__field">
          <input
            type="text"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="sk-ant-..."
          />
        </div>
        <p className="setup-wizard__note">
          The key will NOT be stored in config.toml. Set it as{" "}
          <code>ANTHROPIC_API_KEY</code> in your environment instead.
        </p>
        <div className="setup-wizard__actions">
          <button
            type="button"
            className="intent-reset"
            onClick={() => {
              setProvider(null);
              setStep("select");
            }}
          >
            Back
          </button>
          <button type="button" onClick={handleContinueToConfig}>
            Continue
          </button>
        </div>
        <button type="button" className="setup-wizard__skip" onClick={onDismiss}>
          Skip setup
        </button>
      </section>
    );
  }

  // ----- Configuration -----
  if (step === "configure") {
    return (
      <section className="pane setup-wizard">
        <h2>Create config.toml</h2>
        <p className="setup-wizard__subtitle">
          Create the file <code>{CONFIG_PATH}</code> with this content:
        </p>
        <pre className="setup-wizard__config">
          <code>{configContent}</code>
        </pre>
        <div className="setup-wizard__actions">
          <button
            type="button"
            className="intent-reset"
            onClick={() => handleCopy(configContent)}
          >
            {copyFailed ? "Copy failed" : copied ? "Copied" : "Copy to clipboard"}
          </button>
          <button type="button" onClick={handleContinueToDone}>
            Continue
          </button>
        </div>
        {provider === "ollama" && (
          <div className="setup-wizard__hint">
            <p>Make sure Ollama is installed and running, then pull a model:</p>
            <pre className="setup-wizard__config">
              <code>ollama pull qwen3:8b</code>
            </pre>
          </div>
        )}
        {provider === "anthropic" && (
          <div className="setup-wizard__hint">
            <p>
              Set the API key in your shell profile or systemd environment:
            </p>
            <pre className="setup-wizard__config">
              <code>export ANTHROPIC_API_KEY="sk-ant-..."</code>
            </pre>
          </div>
        )}
        <button type="button" className="setup-wizard__skip" onClick={onDismiss}>
          Skip setup
        </button>
      </section>
    );
  }

  // ----- Done -----
  return (
    <section className="pane setup-wizard">
      <h2>Setup complete</h2>
      <p className="setup-wizard__subtitle">
        Restart the shell to apply the new configuration. LACS will read{" "}
        <code>{CONFIG_PATH}</code> on startup and use{" "}
        {provider === "anthropic" ? "Anthropic" : "Ollama"} as the LLM provider.
      </p>
      <div className="setup-wizard__actions">
        <button type="button" onClick={onDismiss}>
          Done
        </button>
      </div>
    </section>
  );
}
