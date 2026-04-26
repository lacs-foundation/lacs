# Quick Start

> **Recommended:** use SysKnife through Claude Code, Claude Desktop,
> Cursor, or Codex CLI via the [MCP Server](mcp.md). You get the full
> plan/approve/execute loop without leaving your AI assistant.

## Prerequisites

- Rust stable (`rustup update stable`)
- An LLM provider: Ollama (local) or an API key for
  Anthropic / OpenAI / Gemini

## Install

```sh
git clone https://github.com/lacs-foundation/sysknife
cd sysknife
make build
sudo make install
sudo systemctl enable --now sysknife-daemon
```

## Configure your LLM

**Ollama (recommended — local, no API key):**

```sh
ollama pull llama3.2
# SysKnife auto-detects Ollama when no cloud API key is set.
```

**Anthropic:**

```sh
export ANTHROPIC_API_KEY=sk-ant-...
```

**OpenAI:**

```sh
export OPENAI_API_KEY=sk-...
```

**Optional config file** (`~/.config/sysknife/config.toml`):

```toml
[llm]
provider = "ollama"
model    = "llama3.2"
```

## First run

```sh
# Try the planner without executing anything:
sysknife --dry-run "show disk usage"

# Full run (requires daemon):
sysknife "what packages do I have installed as layers?"
```

## Try without installing

```sh
# Plan any intent — no daemon, no approval, no execution.
export ANTHROPIC_API_KEY=sk-ant-...
sysknife --dry-run "show disk usage"
```

Works on any Linux. Prints the typed plan and exits.
