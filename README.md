# LACS — Linux Agent Control Standard

**Describe what you want in plain language. Review a typed plan with
risk levels. Approve explicitly. Watch it execute with live output
and automatic rollback.**

Every action is shown to you before it runs.
The AI cannot do anything you did not review and approve.
Every execution is logged to a local SQLite audit trail.

<!-- TODO: replace with actual demo GIF once recorded on real hardware -->
<!-- ![LACS demo](docs/assets/demo.gif) -->

## Why not just use

| Tool | The problem |
| --- | --- |
| Open Interpreter | Runs arbitrary shell commands. No approval gate. No audit log. |
| Claude Computer Use | Uncontrolled desktop automation, not system administration. |
| Ansible | Requires YAML playbooks written in advance. Not conversational. |
| shell-gpt / Copilot | Suggests raw shell commands for you to paste. You are still running raw shell. |
| Manual commands | No audit trail. No rollback. One typo can be destructive. |

LACS is different: the agent proposes **typed actions** with risk levels
and previews. You review them with full context. A privileged daemon
handles execution with an audit trail and automatic rollback. The AI
never touches the system directly.

## How it works

```text
lacs-brain  →  lacs-shell  →  lacs-daemon
 (planner)      (approval)     (executor)
```

1. You type a natural-language request in the shell
2. The brain proposes a plan — each step is a typed action with a risk
   level (`Low`, `Medium`, `High`)
3. The shell shows the plan with previews and rollback metadata
4. You approve each step explicitly
5. The daemon executes, streams live output, and rolls back automatically
   if a high-risk action fails
6. Every execution is logged to a local SQLite audit trail

The brain proposes but cannot touch the system. The shell renders and
captures approval. The daemon is the only privileged process — it
enforces policy, executes typed actions, writes the transaction log,
and triggers rollback when things go wrong.

## Current status

The core trust chain is built, tested, and wired end-to-end.
Security hardening and multi-distro support are the active milestones.

| Component | Status |
| --- | --- |
| `lacs-brain` — LLM planner, tool loop, safety fence | complete |
| `lacs-daemon` — 60+ typed actions, auth, preview, transactions | complete |
| `lacs-daemon` — IPC dispatcher, live streaming, automatic rollback | complete |
| `lacs-shell` — intent, plan, approval gate, job timeline | complete |
| daemon ↔ shell IPC (length-prefixed JSON over Unix socket) | complete |
| systemd unit, polkit, sysusers, tmpfiles, Makefile | complete |
| `~/.config/lacs/config.toml` support | complete |
| AppImage + RPM + Flatpak bundles | complete |
| Role-to-action allowlist, structured audit log | complete |
| Multi-distro support (apt, dnf, pacman) | roadmap |

230+ tests pass across Rust and TypeScript.

## Quick start

**Prerequisites:** Rust stable, pnpm, [Tauri prerequisites][tauri-prereqs]

[tauri-prereqs]: https://tauri.app/start/prerequisites/

```sh
git clone https://github.com/lacs-foundation/lacs
cd lacs
make build
sudo make install
sudo systemctl enable --now lacs-daemon
```

Launch the shell:

```sh
cd apps/lacs-shell && pnpm install && pnpm tauri dev
```

### LLM provider

LACS works with **Ollama** (no API key, recommended for getting started)
or **Anthropic**.

**Ollama:**

```sh
ollama pull llama3.2
# LACS auto-detects Ollama when ANTHROPIC_API_KEY is not set.
```

**Anthropic:**

```sh
export ANTHROPIC_API_KEY=sk-ant-...
```

**Optional config file** (`~/.config/lacs/config.toml`):

```toml
[llm]
provider = "ollama"
model    = "llama3.2"

[daemon]
socket   = "/run/lacs/daemon.sock"
database = "/var/lib/lacs/daemon.sqlite"
```

Config file values act as defaults. Environment variables always win.

| Variable | Default | Description |
| --- | --- | --- |
| `LACS_LLM_PROVIDER` | auto-detect | `anthropic` or `ollama` |
| `ANTHROPIC_API_KEY` | — | Required when provider is `anthropic` |
| `LACS_LLM_MODEL` | provider default | Override the model name |
| `LACS_OLLAMA_URL` | `http://localhost:11434` | Ollama base URL |
| `LACS_BRAIN_MAX_TURNS` | `5` | Planning turn limit (minimum 1) |

## Building from source

```sh
# Clone
git clone https://github.com/lacs-foundation/lacs
cd lacs

# Run all Rust tests
cargo test --workspace

# Run frontend tests
cd apps/lacs-shell && pnpm install && pnpm test
```

Run the full stack locally:

```sh
# Terminal 1 — daemon (privileged actions require root)
cargo run -p lacs-daemon

# Terminal 2 — shell
cd apps/lacs-shell && pnpm install && pnpm tauri dev
```

See [docs/developer-guide.md](docs/developer-guide.md) for the full
development setup including pre-commit hooks.

## Contributing

Issues tagged `good first issue` are well-scoped with clear acceptance
criteria. Issues tagged `security` take priority.

**Areas where contributions have high impact:**

- Multi-distro action families (apt / dnf / pacman)
- Integration test hardening against a real daemon socket
- Demo recording on real Silverblue hardware

See [CONTRIBUTING.md](CONTRIBUTING.md) for workflow, branch conventions,
and PR checklist. Open an issue before starting any substantial change.

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
