# SysKnife — Linux Agent Control Standard

**Describe what you want in plain language. Review a typed plan with risk levels. Approve explicitly. Watch it execute.**

SysKnife never runs a shell command. Every action is a typed operation with a formal risk level. The AI cannot touch your system directly.

SysKnife is the reference implementation of the [LACS (Linux Agent Control Standard)](https://github.com/lacs-foundation/specification) protocol — an open, CC0-licensed specification for AI agents that operate at the Linux system level. Any implementation that conforms to LACS provides the same safety guarantees: typed actions, formal risk enforcement, mandatory approval gates, immutable audit trail.

```sh
# Try it — no daemon, no execution, no risk.
export ANTHROPIC_API_KEY=sk-ant-...
sysknife --dry-run "show disk usage"
```

---

## The problem with every other tool

Every AI agent that can touch your system has the same flaw: you find out what it did after. Open Interpreter runs arbitrary Python and shell. Goose by Block pops confirmation dialogs. Neither has a formal model of what "safe" means.

**SysKnife is different.**

The AI proposes a **typed plan**. Every step is a named action — not a shell command — with a formal risk level:

- **Low** — read-only. Executes automatically.
- **Medium** — reversible change. Requires your approval.
- **High** — irreversible or access-control. Requires typed confirmation.

You see the plan before anything happens. If you decline, nothing runs. The AI cannot override this.

---

## How it works

```
sysknife-brain  →  sysknife-shell  →  sysknife-daemon
 (planner)      (approval)     (executor)
```

1. You type a natural-language request
2. The brain proposes a plan — typed actions with risk levels
3. The shell renders the plan with previews
4. You approve
5. The daemon executes with live output and automatic rollback
6. Every execution is logged to a local SQLite audit trail

The brain proposes but **cannot touch the system**. The daemon is the only privileged process.

---

## Use from Claude Desktop (MCP)

SysKnife exposes an MCP server so Claude Desktop and Cursor can call the planner directly:

```json
{
  "mcpServers": {
    "sysknife": { "command": "sysknife", "args": ["mcp-server"] }
  }
}
```

---

## Quick start

```sh
git clone https://github.com/lacs-foundation/sysknife
cd sysknife
make build
sudo make install
sudo systemctl enable --now sysknife-daemon
sysknife "show disk usage"
```

See the [Developer Guide](developer-guide.md) for the full setup.

---

## Status

230+ tests. 54 executable E2E user stories. Fedora Silverblue / Atomic Desktop fully supported.

Multi-distro (apt, dnf, pacman), Telegram approval interface, and `sysknife audit export` are on the [roadmap](https://github.com/lacs-foundation/sysknife/blob/main/ROADMAP.md).
