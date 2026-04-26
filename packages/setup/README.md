# sysknife-setup

Zero-friction onboarding wizard for the SysKnife MCP server.

## Usage

```sh
npx sysknife-setup
```

Run from the root of your project. The wizard is interactive and scriptable
via stdin redirection.

## What gets written

The wizard asks which AI clients to configure (all three selected by default).

### Claude Code

| File | Purpose |
|------|---------|
| `.mcp.json` | MCP server config — `{ "mcpServers": { "sysknife": { ... } } }` |
| `.claude/hookify.require-sysknife-approval.local.md` | Approval gate rule |
| `.claude/hookify.sysknife-schema-fetch.local.md` | Deferred-schema reminder |
| `.claude/hookify.sysknife-bash-guard.local.md` | VM query guard |

### Cursor

| File | Purpose |
|------|---------|
| `.cursor/mcp.json` | MCP server config — same `mcpServers` JSON shape as Claude Code |
| `.cursor/rules/sysknife.mdc` | Cursor project rule (approval + safety guidance) |

### Codex CLI (openai/codex)

| File | Purpose |
|------|---------|
| `~/.codex/config.toml` | `[mcp_servers.sysknife]` block appended to global config |
| `AGENTS.md` | SysKnife rules appended (or file created) |

All files that may contain API keys are created with `chmod 0600`.

## Gitignore advice

The wizard prints a reminder to add sensitive files to `.gitignore`:

```
.mcp.json
.claude/*.local.md
.cursor/mcp.json
```

`~/.codex/config.toml` lives outside the project and is not an issue.

## Multi-VM (fleet) mode

When you answer "Y" to "Add another VM?", the wizard collects multiple daemon
socket addresses and names them. Each target becomes a separate MCP server
entry (`sysknife-web`, `sysknife-db`, …) across all written config files.

## Options

```
--help, -h    Show help and exit
```
