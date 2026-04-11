import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { SetupWizard } from "./SetupWizard";

describe("SetupWizard", () => {
  it("renders provider selection by default", () => {
    render(<SetupWizard onDismiss={() => {}} />);
    expect(screen.getByText("Choose your LLM provider")).toBeInTheDocument();
    expect(screen.getByText("Ollama")).toBeInTheDocument();
    expect(screen.getByText("Anthropic")).toBeInTheDocument();
  });

  it("selecting Ollama shows config content", () => {
    render(<SetupWizard onDismiss={() => {}} />);
    fireEvent.click(screen.getByText("Ollama"));
    expect(screen.getByRole("heading", { name: /config\.toml/ })).toBeInTheDocument();
    expect(screen.getByText(/provider.*=.*"ollama"/)).toBeInTheDocument();
  });

  it("selecting Anthropic shows API key input", () => {
    render(<SetupWizard onDismiss={() => {}} />);
    fireEvent.click(screen.getByText("Anthropic"));
    expect(screen.getByPlaceholderText(/sk-ant-/)).toBeInTheDocument();
  });

  it("Anthropic step generates config with API key instruction", () => {
    render(<SetupWizard onDismiss={() => {}} />);
    fireEvent.click(screen.getByText("Anthropic"));
    fireEvent.change(screen.getByPlaceholderText(/sk-ant-/), {
      target: { value: "sk-ant-test-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: /continue/i }));
    expect(screen.getByRole("heading", { name: /config\.toml/ })).toBeInTheDocument();
    expect(screen.getByText(/provider.*=.*"anthropic"/)).toBeInTheDocument();
  });

  it("Done step calls onDismiss", () => {
    const onDismiss = vi.fn();
    render(<SetupWizard onDismiss={onDismiss} />);
    // Go through Ollama flow
    fireEvent.click(screen.getByText("Ollama"));
    fireEvent.click(screen.getByRole("button", { name: /continue/i }));
    // Now on Done step
    fireEvent.click(screen.getByRole("button", { name: /done/i }));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("skip setup link calls onDismiss", () => {
    const onDismiss = vi.fn();
    render(<SetupWizard onDismiss={onDismiss} />);
    fireEvent.click(screen.getByText("Skip setup"));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("shows 'Copy failed' when clipboard write rejects", async () => {
    // Stub clipboard to reject
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockRejectedValue(new Error("Not allowed")) },
    });

    render(<SetupWizard onDismiss={() => {}} />);
    fireEvent.click(screen.getByText("Ollama"));

    const copyBtn = screen.getByRole("button", { name: /copy to clipboard/i });
    fireEvent.click(copyBtn);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /copy failed/i })).toBeInTheDocument();
    });
  });

  it("shows 'Copied' when clipboard write succeeds", async () => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    });

    render(<SetupWizard onDismiss={() => {}} />);
    fireEvent.click(screen.getByText("Ollama"));

    const copyBtn = screen.getByRole("button", { name: /copy to clipboard/i });
    fireEvent.click(copyBtn);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /copied/i })).toBeInTheDocument();
    });
  });
});
