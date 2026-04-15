# Quick Start

## Prerequisites

- Rust stable (`rustup update stable`)
- pnpm (`npm install -g pnpm`)
- [Tauri prerequisites](https://tauri.app/start/prerequisites/)
- An LLM provider: Ollama (local) or an API key for Anthropic / OpenAI / Gemini

## Install

```sh
git clone https://github.com/lacs-foundation/lacs
cd lacs
make build
sudo make install
sudo systemctl enable --now lacs-daemon
```

## Configure your LLM

**Ollama (recommended — local, no API key):**
```sh
ollama pull llama3.2
# LACS auto-detects Ollama when no cloud API key is set.
```

**Anthropic:**
```sh
export ANTHROPIC_API_KEY=sk-ant-...
```

**OpenAI:**
```sh
export OPENAI_API_KEY=sk-...
```

**Optional config file** (`~/.config/lacs/config.toml`):
```toml
[llm]
provider = "ollama"
model    = "llama3.2"
```

## First run

```sh
# Try the planner without executing anything:
lacs --dry-run "show disk usage"

# Full run (requires daemon):
lacs "what packages do I have installed as layers?"
```

## Try without installing

```sh
# Plan any intent — no daemon, no approval, no execution.
export ANTHROPIC_API_KEY=sk-ant-...
lacs --dry-run "show disk usage"
```

Works on any Linux. Prints the typed plan and exits.
