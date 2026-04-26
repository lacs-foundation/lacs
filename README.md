<p align="center">
  <a href="https://github.com/lacs-foundation/sysknife">
    <img src="assets/logo/sysknife.svg" alt="SysKnife" width="170" height="170"/>
  </a>
</p>

<h1 align="center">SysKnife</h1>

<p align="center">
  <em>Your sysadmin co-pilot. Plan. Approve. Audit.</em>
</p>

<p align="center">
  <a href="https://github.com/lacs-foundation/sysknife/actions"><img src="https://img.shields.io/github/actions/workflow/status/lacs-foundation/sysknife/ci.yml?branch=main&style=flat-square&logo=github&label=CI" alt="CI"></a>
  <a href="https://github.com/lacs-foundation/sysknife/blob/main/LICENSE"><img src="https://img.shields.io/github/license/lacs-foundation/sysknife?style=flat-square" alt="License"></a>
  <a href="https://github.com/lacs-foundation/sysknife/stargazers"><img src="https://img.shields.io/github/stars/lacs-foundation/sysknife?style=flat-square&logo=github" alt="Stars"></a>
  <a href="https://github.com/lacs-foundation/sysknife/issues"><img src="https://img.shields.io/github/issues/lacs-foundation/sysknife?style=flat-square" alt="Issues"></a>
  <a href="https://github.com/lacs-foundation/sysknife/discussions"><img src="https://img.shields.io/github/discussions/lacs-foundation/sysknife?style=flat-square&label=discuss" alt="Discussions"></a>
</p>

<p align="center">
  <strong>Distros</strong>&nbsp;
  <img src="https://img.shields.io/badge/Fedora%2041%2B-✓-294172?style=flat-square&logo=fedora&logoColor=white" alt="Fedora 41+">
  <img src="https://img.shields.io/badge/Silverblue%2041%2B-✓-294172?style=flat-square&logo=fedora&logoColor=white" alt="Silverblue 41+">
  <img src="https://img.shields.io/badge/Ubuntu%2026.04-roadmap-E95420?style=flat-square&logo=ubuntu&logoColor=white" alt="Ubuntu 26.04 roadmap">
  <img src="https://img.shields.io/badge/Ubuntu%2024.04-roadmap-E95420?style=flat-square&logo=ubuntu&logoColor=white" alt="Ubuntu 24.04 roadmap">
  <img src="https://img.shields.io/badge/Ubuntu%2022.04-roadmap-E95420?style=flat-square&logo=ubuntu&logoColor=white" alt="Ubuntu 22.04 roadmap">
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="#why-not-just">Why not <em>X</em>?</a> ·
  <a href="docs/distro-support.md">Distro matrix</a> ·
  <a href="ROADMAP.md">Roadmap</a> ·
  <a href="CONTRIBUTING.md">Contribute</a> ·
  <a href="https://github.com/lacs-foundation/sysknife/discussions">Discuss</a>
</p>

<p align="center">
  <img src="assets/demo/demo.gif" alt="SysKnife demo" width="900"/>
</p>

> **Describe what you want in plain language.** Review a typed plan with risk
> levels. Approve explicitly. Watch it execute with live output and automatic
> rollback — every action signed and audited.

SysKnife never runs a shell command. Every action is a **typed operation**
with a formal risk level. The AI cannot touch your system directly. A
privileged daemon executes only what you approve, writes a tamper-evident
HMAC-SHA256 audit chain, and rolls back automatically if a high-risk step
fails.

---

## Install

The fastest path is the npx-based setup wizard that detects your distro,
provisions the daemon, and writes the MCP config for Claude Code / Cursor /
any MCP-capable client.

```sh
# detects Fedora vs Ubuntu, asks for the LLM provider, sets everything up.
npx @sysknife/setup
```

Manual install per distro:

<details>
<summary><strong>Fedora 41+ / Silverblue 41+</strong></summary>

```sh
# Install
git clone https://github.com/lacs-foundation/sysknife
cd sysknife
make build
sudo make install
sudo systemctl enable --now sysknife-daemon

# Run a dry plan
export OPENAI_API_KEY=sk-...        # or ANTHROPIC_API_KEY / GEMINI_API_KEY
sysknife --dry-run "show disk usage"
```
</details>

<details>
<summary><strong>Ubuntu 26.04 / 24.04 / 22.04</strong> (roadmap — Phase 2)</summary>

Ubuntu support is the active milestone — see
[`ROADMAP.md`](ROADMAP.md) and
[`docs/distro-support.md`](docs/distro-support.md). The action layer is being
abstracted so apt, snap, flatpak, ufw, and netplan are first-class peers
of rpm-ostree on Silverblue. The CLI / shell / MCP server already build and
plan correctly on Ubuntu — only the privileged action set is gated.
</details>

<details>
<summary><strong>Without installing anything (cloud-only dry run)</strong></summary>

```sh
# Plans only. No daemon, no approval, no execution.
export ANTHROPIC_API_KEY=sk-ant-...
sysknife --dry-run "show disk usage and list services that ate cpu in the last hour"
```
</details>

## How it works

```
sysknife-brain   →   sysknife-shell   →   sysknife-daemon
  (planner)         (approval gate)        (executor)
   talks to LLM      shows the plan,        only privileged
   never to OS       takes y/n              process; signs
                                            every action
```

1. You type a natural-language request.
2. The brain proposes a plan — each step is a **typed action** with
   a risk level (`Low` · `Medium` · `High`).
3. The shell shows the plan with previews, side-effects, and rollback
   metadata.
4. You approve each step explicitly (or set `--yes` up to a risk ceiling).
5. The daemon executes, streams live output, rolls back automatically on
   high-risk failure.
6. Every execution is logged to a hash-chained SQLite or Postgres audit
   trail you can verify with `sysknife audit verify`.

The brain *proposes*; only the daemon is privileged. The daemon *enforces*
policy, executes typed actions, writes the chain, and triggers rollback.
The trust boundary is mechanical — no shell strings cross the wire.

## Why not just …?

| Tool | The gap |
|---|---|
| **Open Interpreter** | Runs arbitrary Python/Shell. No formal risk model. No audit chain. |
| **Goose / Continue** | General-purpose. Ad-hoc confirmation, not typed risk levels. |
| **Claude Computer Use** | Uncontrolled desktop automation, not system administration. |
| **Ansible** | YAML written in advance. Not conversational. No risk classification. |
| **shell-gpt / Copilot** | Suggests raw shell commands. You still run raw shell. |
| **Manual** | No audit trail. No rollback. One typo = lost work. |

SysKnife is different by construction: typed actions, signed audit chain,
explicit approval gate, automatic rollback for high-risk paths, polkit-mediated
privilege boundary. The AI never holds a shell.

## Status

The trust chain is built, tested, and shipping. Multi-distro is the active
milestone.

| Component | State |
|---|---|
| `sysknife-brain` — LLM planner, tool loop, safety fence | ✅ |
| `sysknife-daemon` — 60+ typed actions, auth, preview, transactions | ✅ |
| Live IPC + streaming + automatic rollback | ✅ |
| Tauri shell — intent, plan, approval gate | ✅ |
| MCP server (Claude Code / Cursor / any MCP client) | ✅ |
| Tamper-evident HMAC-SHA256 audit chain | ✅ |
| RFC 5424 syslog forwarding (Splunk / Sentinel / QRadar) | ✅ |
| Postgres backend (RDS / Cloud SQL / Neon / Supabase) | ✅ |
| **Ubuntu LTS support (22.04 / 24.04 / 26.04)** | 🛠 active |
| Telegram approval interface | 📋 roadmap |

**860+ tests** pass across Rust and TypeScript on every commit.

## Configure your LLM

SysKnife works with **Ollama** (no key, recommended for privacy / offline /
homelab) or **OpenAI**, **Anthropic**, **Gemini**, **Groq**, **DeepSeek**,
**Mistral**, **xAI**.

```toml
# ~/.config/sysknife/config.toml
[llm]
provider     = "ollama"          # or anthropic / openai / gemini / groq / ...
model        = "qwen3:8b"        # provider-specific
ollama_url   = "http://localhost:11434"
max_turns    = 10

[daemon]
socket   = "/run/sysknife/daemon.sock"
database = "/var/lib/sysknife/daemon.sqlite"

[storage]                         # production-recommended
backend = "postgres"
url     = "postgres://sysknife:${PG_PASSWORD}@db.example.com/audit?sslmode=verify-full"
```

Env vars always win over the config file. Full reference in
[`docs/configuration.md`](docs/configuration.md).

## MCP server

SysKnife exposes an [MCP](https://modelcontextprotocol.io/) server so any
MCP-capable agent (Claude Code, Cursor, etc.) can plan + approve + execute
through SysKnife's risk-gated path. Set up with one command:

```sh
npx @sysknife/setup
# detects sysknife, asks for the daemon socket and LLM provider,
# writes .mcp.json + the Claude Code approval hook.
```

The MCP layer enforces the same approval contract as the CLI: agents must
call `sysknife_plan` first, present the plan, wait for explicit human
approval, then call `sysknife_execute`. High-risk actions are refused
outright at the MCP boundary — they require the CLI/GUI confirmation flow.

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the full milestone breakdown.

- 🛠 **Phase 2 — Ubuntu LTS support (22.04 / 24.04 / 26.04)** [active]
- 📋 Telegram inline-button approvals
- 📋 `sysknife audit export` (CEF / NDJSON for SIEM ingest)
- 📋 Fleet plan/execute (one plan, N targets, parallel approval)
- 📋 GUI (Tauri shell) for Wayland desktop linux

## Protocol

SysKnife is the reference implementation of the **LACS (Linux Agent Control
Standard)** protocol — typed actions, risk classification, approval gates,
audit requirements. The spec is CC0 (public domain):

→ **[lacs-foundation/specification](https://github.com/lacs-foundation/specification)**

Other implementations for other distros and languages are explicitly
encouraged.

## Contributing

We want help. **Multi-distro** is the highest-impact area to plug into right
now — see [`docs/distro-support.md`](docs/distro-support.md) for the
roadmap matrix and [`CONTRIBUTING.md`](CONTRIBUTING.md) for the workflow.

Issues labelled
[`good first issue`](https://github.com/lacs-foundation/sysknife/labels/good%20first%20issue)
are scoped with clear acceptance criteria.

## Documentation

- [Architecture overview](docs/architecture.md)
- [Distro support matrix](docs/distro-support.md)
- [Developer guide](docs/developer-guide.md)
- [Testing guide](docs/contributing/testing.md)
- [VM daemon setup](docs/vm-daemon-setup.md)
- [Security policy](SECURITY.md)
- [Roadmap](ROADMAP.md)
- [ADR 0001 — System boundaries](docs/adr/0001-system-boundaries.md)
- [ADR 0002 — Brain provider layer](docs/adr/0002-brain-provider-layer.md)
- [ADR 0003 — IPC wire protocol](docs/adr/0003-ipc-wire-protocol.md)

## License

MIT. See [LICENSE](LICENSE).

---

<p align="center">
  Built by <a href="https://github.com/vladimirrotariu">Vladimir Rotariu</a>.
  ·
  Issues, ideas, war stories — <a href="https://github.com/lacs-foundation/sysknife/discussions">come say hi</a>.
</p>
