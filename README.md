# SysKnife — Linux Agent Control Standard

**Describe what you want in plain language. Review a typed plan with
risk levels. Approve explicitly. Watch it execute with live output
and automatic rollback.**

SysKnife never runs a shell command. Every action is a typed operation with
a formal risk level. The AI cannot touch your system directly.

Every action is shown to you before it runs.
Every execution is logged to a local SQLite audit trail.

<!-- TODO: replace with actual demo GIF once recorded on real hardware -->
<!-- ![SysKnife demo](docs/assets/demo.gif) -->

## Try it without installing anything

```sh
# Plan any intent — no daemon, no approval, no execution.
export ANTHROPIC_API_KEY=sk-ant-...   # or OPENAI_API_KEY / GEMINI_API_KEY
sysknife --dry-run "show disk usage"
```

Works on any Linux. Prints the typed plan and exits.

## Why not just use

| Tool | Stars | The problem |
|---|---|---|
| Open Interpreter | 63k | Runs arbitrary Python/Shell. No formal risk model. No audit trail. |
| Goose by Block | 29k | General-purpose. Ad-hoc confirmation, not typed risk levels. No sysadmin-first design. |
| Claude Computer Use | — | Uncontrolled desktop automation, not system administration. |
| Ansible | — | Requires YAML playbooks written in advance. Not conversational. |
| shell-gpt / Copilot | — | Suggests raw shell commands for you to paste. You are still running raw shell. |
| Manual commands | — | No audit trail. No rollback. One typo can be destructive. |

SysKnife is different: the agent proposes **typed actions** with risk levels
and previews. You review them with full context. A privileged daemon
handles execution with an audit trail and automatic rollback. The AI
never touches the system directly.

## How it works

```text
sysknife-brain  →  sysknife-shell  →  sysknife-daemon
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
|---|---|
| `sysknife-brain` — LLM planner, tool loop, safety fence | complete |
| `sysknife-daemon` — 60+ typed actions, auth, preview, transactions | complete |
| `sysknife-daemon` — IPC dispatcher, live streaming, automatic rollback | complete |
| `sysknife-shell` — intent, plan, approval gate, job timeline | complete |
| daemon ↔ shell IPC (length-prefixed JSON over Unix socket) | complete |
| systemd unit, polkit, sysusers, tmpfiles, Makefile | complete |
| `~/.config/sysknife/config.toml` support | complete |
| AppImage + RPM + Flatpak bundles | complete |
| Role-to-action allowlist, structured audit log | complete |
| Multi-distro support (apt, dnf, pacman) | roadmap |

230+ tests pass across Rust and TypeScript.

## Quick start

**Prerequisites:** Rust stable, pnpm, [Tauri prerequisites][tauri-prereqs]

[tauri-prereqs]: https://tauri.app/start/prerequisites/

```sh
git clone https://github.com/sysknife-foundation/sysknife
cd sysknife
make build
sudo make install
sudo systemctl enable --now sysknife-daemon
```

Launch the shell:

```sh
cd apps/sysknife-shell && pnpm install && pnpm tauri dev
```

### LLM provider

SysKnife works with **Ollama** (no API key, recommended for getting started)
or with **Anthropic**, **OpenAI**, or **Gemini**.

**Ollama (recommended for privacy and offline use):**

```sh
ollama pull llama3.2
# SysKnife auto-detects Ollama when no cloud API key is set.
```

**Anthropic:**

```sh
export ANTHROPIC_API_KEY=sk-ant-...
```

**Optional config file** (`~/.config/sysknife/config.toml`):

```toml
[llm]
provider = "ollama"
model    = "llama3.2"

[daemon]
socket   = "/run/sysknife/daemon.sock"
database = "/var/lib/sysknife/daemon.sqlite"
```

Config file values act as defaults. Environment variables always win.

| Variable | Default | Description |
|---|---|---|
| `SYSKNIFE_LLM_PROVIDER` | auto-detect | `anthropic`, `openai`, `gemini`, or `ollama` |
| `ANTHROPIC_API_KEY` | — | Required when provider is `anthropic` |
| `OPENAI_API_KEY` | — | Required when provider is `openai` |
| `GEMINI_API_KEY` | — | Required when provider is `gemini` |
| `SYSKNIFE_LLM_MODEL` | provider default | Override the model name |
| `SYSKNIFE_OLLAMA_URL` | `http://localhost:11434` | Ollama base URL |
| `SYSKNIFE_BRAIN_MAX_TURNS` | `5` | Planning turn limit (minimum 1) |

## Building from source

```sh
# Clone
git clone https://github.com/sysknife-foundation/sysknife
cd sysknife

# Run all Rust tests
cargo test --workspace --locked

# Run frontend tests
cd apps/sysknife-shell && pnpm install && pnpm test && cd ../..
```

Run the full stack locally:

```sh
# Terminal 1 — daemon (privileged actions require root)
cargo run -p sysknife-daemon

# Terminal 2 — shell
cd apps/sysknife-shell && pnpm install && pnpm tauri dev
```

See [docs/developer-guide.md](docs/developer-guide.md) for the full
development setup including pre-commit hooks and E2E story testing.

## MCP server

SysKnife exposes an [MCP](https://modelcontextprotocol.io/) server so Claude
Desktop, Cursor, and any other MCP-capable agent can invoke the planner
directly.

Add to `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "sysknife": {
      "command": "sysknife",
      "args": ["mcp-server"]
    }
  }
}
```

The server exposes a single `lacs_plan` tool. The calling agent passes a
natural-language intent; SysKnife returns a typed plan with risk levels. The
approval flow is preserved at the MCP layer.

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the full milestone breakdown.

Upcoming highlights:

- **Multi-distro** — apt (Debian/Ubuntu), dnf (Fedora Workstation), pacman (Arch)
- **Telegram interface** — approve SysKnife plans from your phone via inline buttons
- **`sysknife audit export`** — export execution history as JSON for analysis

## Contributing

Contributions are welcome. Issues tagged `good first issue` are
well-scoped with clear acceptance criteria — a great place to start.

**Areas where contributions have high impact:**

- Multi-distro action families (apt / dnf / pacman)
- Integration test hardening against a real daemon socket
- Demo recording on real Silverblue hardware

See [CONTRIBUTING.md](CONTRIBUTING.md) for workflow, branch conventions,
and PR checklist. Open an issue before starting any substantial change.

## Documentation

- [Architecture overview](docs/architecture.md)
- [Developer guide](docs/developer-guide.md)
- [Testing guide](docs/contributing/testing.md)
- [Contributing guide](docs/contributing/CONTRIBUTING.md)
- [Roadmap](ROADMAP.md)
- [ADR 0001: System boundaries](docs/adr/0001-system-boundaries.md)
- [ADR 0002: Brain provider layer](docs/adr/0002-brain-provider-layer.md)
- [ADR 0003: IPC wire protocol](docs/adr/0003-ipc-wire-protocol.md)
- [Security policy](SECURITY.md)

## License

MIT. See [LICENSE](LICENSE).

---

Built by [Vladimir Rotariu](https://github.com/vladimirrotariu).
