# LACS — Linux Agent Control Standard

An AI agent for Linux that shows you exactly what it intends to do
and asks for your approval before changing anything.

You describe what you want in plain language.
LACS proposes a typed plan with risk levels, previews, and rollback
metadata.
You approve each step explicitly.
The daemon executes, streams live output, and rolls back automatically
if a high-risk action fails.
Every execution is logged to a local SQLite audit trail.

## Why LACS

| Tool | The problem |
| --- | --- |
| Open Interpreter | Runs arbitrary shell commands. No approval gate. No audit log. Dangerous on servers. |
| Claude Computer Use | Uncontrolled. Designed for desktop automation, not system administration. |
| Ansible | Requires YAML playbooks written in advance. Not conversational. You need to know what you want before you start. |
| shell-gpt / Copilot | Suggests raw shell commands for you to paste into a terminal. You are still reviewing and running raw shell. |
| Manual commands | No audit trail. No rollback. One typo can be destructive. |

LACS occupies a different position: the agent proposes typed actions,
you review them with full context, and a privileged daemon handles
execution with an audit trail and automatic rollback.
The AI cannot do anything you did not explicitly review and approve.

## Current Status

The core trust chain is built, tested, and wired end-to-end.
Security hardening and multi-distro support are the next milestones.

| Component | Status |
| --- | --- |
| `lacs-brain` — LLM planner, tool loop, safety fence | complete |
| `lacs-daemon` — 60+ typed actions, auth, preview, transactions | complete |
| `lacs-daemon` — IPC dispatcher, live streaming, automatic rollback | complete |
| `lacs-shell` — intent, plan, approval gate, job timeline | complete |
| daemon ↔ shell IPC | complete |
| systemd unit, polkit, install script | complete |
| `~/.config/lacs/config.toml` support | complete |
| AppImage + RPM + Flatpak bundles | complete |
| multi-distro support (apt, dnf, pacman) | roadmap |

230+ tests pass across Rust and TypeScript.

## Install

**Requires:** Rust stable, pnpm, Tauri prerequisites.

```sh
git clone https://github.com/lacs-foundation/lacs
cd lacs
make build
sudo make install
sudo systemctl enable --now lacs-daemon
```

Then launch the shell:

```sh
cd apps/lacs-shell && pnpm install && pnpm tauri dev
```

**Ollama (no API key):**

```sh
ollama pull llama3.2
# The shell auto-detects Ollama when ANTHROPIC_API_KEY is not set.
```

**Optional config file** (`~/.config/lacs/config.toml`):

```toml
[llm]
provider = "ollama"
model    = "llama3.2"
```

## Architecture

```text
lacs-brain  →  lacs-shell  →  lacs-daemon
 (planner)      (approval)     (executor)
```

The brain proposes plans but cannot touch the system directly.
The shell renders the plan, captures your approval, and streams
progress back to you.
The daemon is the only privileged process — it enforces policy,
executes typed actions, writes the transaction log, and triggers
rollback when things go wrong.

Read [docs/architecture.md](docs/architecture.md) for more detail.

## Building From Source

**Prerequisites:**

- Rust stable ([rustup.rs](https://rustup.rs))
- Node.js 20 ([nodejs.org](https://nodejs.org))
- Tauri prerequisites ([tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/))

```sh
# Clone
git clone https://github.com/lacs-foundation/lacs
cd lacs

# Run Rust tests
cargo test --workspace

# Run frontend tests
cd apps/lacs-shell && pnpm install && pnpm test
```

To run the full stack locally:

```sh
# Terminal 1 — start the daemon (privileged actions require root)
cargo run -p lacs-daemon

# Terminal 2 — start the shell
cd apps/lacs-shell && pnpm install && pnpm tauri dev
```

See [docs/developer-guide.md](docs/developer-guide.md) for the full
development setup.

## LLM Configuration

`lacs-brain` resolves its provider from environment variables.
The Ollama path works without any API key and is the recommended
starting point for local development.

| Variable | Default | Description |
| --- | --- | --- |
| `LACS_LLM_PROVIDER` | auto-detect | `anthropic` or `ollama` |
| `ANTHROPIC_API_KEY` | — | Required when provider is `anthropic` |
| `LACS_LLM_MODEL` | provider default | Override the model name |
| `LACS_OLLAMA_URL` | `http://localhost:11434` | Ollama base URL |
| `LACS_ANTHROPIC_URL` | `https://api.anthropic.com` | Anthropic base URL |
| `LACS_BRAIN_MAX_TURNS` | `5` | Planning turn limit (minimum 1) |

**Ollama (no API key required):**

```sh
ollama pull llama3.2
cargo run -p lacs-daemon &
cd apps/lacs-shell && npm run tauri dev
```

**Anthropic:**

```sh
export ANTHROPIC_API_KEY=sk-ant-...
cargo run -p lacs-daemon &
cd apps/lacs-shell && npm run tauri dev
```

## Contributing

We welcome contributors interested in any of:

- Rust systems programming and daemon work
- Linux packaging and distribution (apt, dnf, pacman, RPM, Flatpak)
- Tauri and React UI development
- systemd service design and release engineering
- Security hardening and audit tooling
- Documentation and developer experience

**High-impact areas open now:**

- Security hardening — role-to-action allowlists, structured audit logging
- Multi-distro action families (apt / dnf / pacman)
- UX polish — reconnect banner, risk-scaled confirmation, execution timeline
- First-run experience and LLM provider setup wizard

See [CONTRIBUTING.md](CONTRIBUTING.md) to get started.
Open an issue before starting any substantial change.

## Documentation

- [Architecture overview](docs/architecture.md)
- [Developer guide](docs/developer-guide.md)
- [Roadmap](ROADMAP.md)
- [ADR 0001: System boundaries](docs/adr/0001-system-boundaries.md)
- [ADR 0002: Brain provider layer](docs/adr/0002-brain-provider-layer.md)
- [ADR 0003: IPC wire protocol](docs/adr/0003-ipc-wire-protocol.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)

## License

MIT. See [LICENSE](LICENSE).
