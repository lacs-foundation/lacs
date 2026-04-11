# LACS

LACS is the Linux Agent Control Standard: a local, typed,
rollback-aware control plane for Linux systems.

It is designed for power users, system administrators, and
contributors who want to build a safe OSS platform for real system
control without giving an agent arbitrary shell access or root.

## What This Project Is

LACS is not a generic chatbot, not a browser automation framework,
and not a desktop replacement.
It is a privileged Linux execution layer with:

- typed actions
- explicit previews
- approval gating
- audit logs
- rollback metadata
- a strict boundary between planner, UI, and executor

## Current Status

LACS is under active development. 176 Rust tests pass. The brain
planning layer is complete, and the daemon IPC foundation (framing,
state collection, action execution, transaction tracking) is
implemented. The dispatcher and shell client are the next milestone.

| Component | Status |
| --- | --- |
| `lacs-brain` — LLM planner, tool loop, safety fence | **complete** |
| `lacs-daemon` — action families, policy, previews, transactions | **complete** |
| `lacs-daemon` — IPC framing, executor, state collector | **complete** |
| `lacs-daemon` — dispatcher (connection handler, job loop) | in progress |
| `lacs-shell` — intent, plan, approval, timeline UI | scaffolded |
| daemon ↔ shell IPC | in progress |

Reference docs:

- [Specification draft](docs/plans/2026-04-10-lacs-spec.md)
- [Daemon IPC spec](docs/plans/2026-04-10-lacs-daemon-ipc-spec.md)

## Architecture at a Glance

- `lacs-brain` (agent identity: `zeroclaw-brain`): unprivileged planner
- `lacs-shell`: user-facing control surface
- `lacs-daemon`: trusted privileged executor

The daemon owns policy, authorization, preview generation, execution,
jobs, transactions, and rollback metadata.
The shell renders intent, preview, approval, progress, and history.
The brain proposes plans but does not mutate the system directly.

## Who Should Contribute

We welcome contributors who care about:

- Linux systems and admin workflows
- Rust and typed APIs
- Fedora Silverblue and transactional systems
- safety, auditability, and rollback
- Tauri UI work
- packaging, CI, documentation, and release engineering

## How To Get Started

1. Read the spec.
2. Read the [daemon IPC spec](docs/plans/2026-04-10-lacs-daemon-ipc-spec.md) for active work.
3. Open an issue for any substantial change.
4. Keep pull requests small and reviewable.
5. Preserve the trust boundary: planner, shell, and daemon stay separate.

## Configuring the Brain (LLM Provider)

`lacs-brain` resolves its LLM provider from environment variables.
Without any config, it defaults to a local Ollama instance.

| Variable | Default | Description |
| --- | --- | --- |
| `LACS_LLM_PROVIDER` | auto-detect | `anthropic` or `ollama` |
| `ANTHROPIC_API_KEY` | — | Required when provider is `anthropic` |
| `LACS_ANTHROPIC_URL` | `https://api.anthropic.com` | Anthropic base URL |
| `LACS_LLM_MODEL` | provider default | Override model name |
| `LACS_OLLAMA_URL` | `http://localhost:11434` | Ollama base URL |
| `LACS_BRAIN_MAX_TURNS` | `5` | Turn limit (must be >= 1) |

**Quick start with Anthropic:**

```sh
export ANTHROPIC_API_KEY=sk-ant-...
cargo run -p lacs-shell
```

**Quick start with Ollama (local model):**

```sh
# Start ollama with a model that supports tool use, e.g. llama3.2
ollama pull llama3.2
cargo run -p lacs-shell
```

## Contribution Standards

- Prefer small, focused pull requests.
- Document user-visible behavior.
- Add or update tests for behavior changes.
- Keep privileged operations typed and bounded.
- Preserve rollback and transaction history for every mutating action.

## Roadmap

The near-term roadmap is documented in [ROADMAP.md](ROADMAP.md).

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).
