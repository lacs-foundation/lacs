# Developer Guide

This guide explains how to work on LACS as a contributor.

## Read First

- [Architecture overview](architecture.md)
- [Specification draft](plans/2026-04-10-lacs-spec.md)
- [Daemon IPC spec](plans/2026-04-10-lacs-daemon-ipc-spec.md)
- [ADR 0001: System boundaries](adr/0001-system-boundaries.md)

## Prerequisites

- **Rust stable** — install via [rustup.rs](https://rustup.rs).
  The workspace uses the `stable` toolchain.
- **Node.js 20** — install via [nodejs.org](https://nodejs.org) or
  your distro package manager.
- **Tauri system dependencies** — see
  [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/)
  for the native libraries required on your distro.

No API key is required for development. The brain falls back to a
local Ollama instance by default.

## Repository Layout

- `crates/lacs-brain/` — unprivileged LLM planner; Anthropic and
  Ollama providers, tool-use loop, plan validation. Complete.
- `crates/lacs-daemon/` — privileged executor; 60+ typed actions,
  role-based auth, preview, IPC dispatcher, live streaming, rollback,
  SQLite transaction log. Complete.
- `crates/lacs-core/` — shared constants (default socket path,
  database path).
- `crates/lacs-proto/` — protobuf definitions; kept for potential
  future use. Current IPC encoding is length-prefixed JSON.
- `crates/lacs-types/` — shared domain types (`CallerRole`,
  `RiskLevel`, `JobState`, request and result envelopes).
- `apps/lacs-shell/` — Tauri + React control surface; intent,
  plan, approval gate, live job timeline. Complete.
- `docs/plans/` — specification and implementation reference docs.
- `docs/adr/` — architectural decision records.

## Running Locally

**Run all tests (no network required):**

```sh
cargo test --workspace
cd apps/lacs-shell && npm install && npm test
```

**Run the full stack:**

```sh
# Terminal 1 — start the daemon
# Privileged system actions require root; safe for development without
cargo run -p lacs-daemon

# Terminal 2 — start the shell
cd apps/lacs-shell
npm install
npm run tauri dev
```

**Run only the daemon with a test client:**

The IPC protocol is human-readable JSON over a Unix socket. You can
send requests manually with `socat`:

```sh
cargo run -p lacs-daemon &
socat - UNIX-CONNECT:/tmp/lacs-daemon.sock
```

**Environment variables:**

| Variable | Default | Description |
| --- | --- | --- |
| `LACS_LISTEN_URI` | `unix:///tmp/lacs-daemon.sock` | Daemon socket path |
| `LACS_DATABASE_PATH` | `/tmp/lacs-daemon.sqlite` | SQLite database path |
| `LACS_LLM_PROVIDER` | auto-detect | `anthropic` or `ollama` |
| `ANTHROPIC_API_KEY` | — | Required for the Anthropic provider |
| `LACS_OLLAMA_URL` | `http://localhost:11434` | Ollama base URL |

## Working Style

- keep changes small and reviewable
- keep behavior typed and explicit
- keep the daemon as the only privileged executor
- update docs when user-facing behavior changes
- add or update tests for every behavior change

## Contribution Workflow

1. Open or find an issue.
2. Align the change with the architecture and trust boundary.
3. Make the smallest implementation that solves the problem.
4. Write the failing test first; verify it fails before adding code.
5. Update docs if user-facing behavior changed.
6. Submit a focused PR.

## Quality Bar

Before merging, a change should be:

- understandable without reading every dependency
- covered by deterministic tests
- documented if it changes user-visible behavior
- safe by default (fail closed, not open)
- consistent with the trust boundary (daemon is the only executor)

## CI

CI runs on every pull request and push to `main`.

| Check | Command |
| --- | --- |
| Rust formatting | `cargo fmt --all --check` |
| Clippy (warnings as errors) | `cargo clippy --workspace --all-features --locked -- -D warnings` |
| Rust tests | `cargo test --workspace --locked` |
| TypeScript type check | `npx tsc --noEmit` (in `apps/lacs-shell`) |
| Frontend tests | `npm test` (in `apps/lacs-shell`) |
| Markdown lint | `markdownlint-cli2` on contributor-facing docs |
| Link check | `markdown-link-check` on contributor-facing docs |
| YAML lint | `yamllint` on issue templates and workflows |

Run the Rust checks locally before pushing:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-features --locked -- -D warnings
cargo test --workspace --locked
```
