# `lacs` CLI Reference

`lacs` is the command-line interface to the LACS daemon.  It turns a
natural-language intent into a risk-labelled plan, asks for approval where
needed, and streams execution output in real time.

---

## Quick start

```sh
# Check that the daemon is reachable
lacs doctor

# Plan + execute a single intent
lacs "check disk usage"

# Preview the plan without executing
lacs --dry-run "list running containers"

# Open the interactive REPL
lacs
```

---

## Synopsis

```
lacs [GLOBAL FLAGS] [SUBCOMMAND | INTENT WORDS...]
```

When no subcommand is given and no intent words are provided, `lacs` starts
an interactive REPL.

---

## Subcommands

### `lacs <intent>`

Plan and (optionally) execute a natural-language intent.

```sh
lacs "check disk usage"
lacs check disk usage            # words are joined — same result
lacs "list running containers"
lacs "is firewalld active?"
lacs "layer vim via rpm-ostree"
```

**What happens:**

1. A spinner appears while the LLM plans (`Thinking…` → `Querying …` →
   `Proposing plan…`).
2. The coloured plan is printed — each step shows a risk badge
   (`● low` / `● medium` / `● HIGH`), the action name, and a summary.
3. If any step requires approval, you are prompted.  HIGH-risk steps always
   require confirmation regardless of `--yes`.
4. Execution streams output line by line with a `›` prefix; a `✓` / `✗`
   result icon is printed after each step.

---

### `lacs doctor`

Check daemon connectivity and print the resolved configuration.

```sh
lacs doctor
lacs --json doctor      # machine-readable
```

Exit code `0` on success, non-zero if the daemon is unreachable.

Sample output:

```
✓  daemon ok
  socket    /run/lacs/lacs.sock
  host      my-silverblue
  provider  anthropic
  model     claude-sonnet-4-6
```

---

### `lacs history`

Query past LACS execution history.

```sh
lacs history
lacs history --limit 50
lacs history --status failed
lacs history --action InstallPackages
lacs history --since 2026-04-01T00:00:00Z
lacs history --status succeeded --limit 5 --since 2026-04-10T00:00:00Z
```

**Flags:**

| Flag | Default | Description |
|---|---|---|
| `--limit N` | `20` | Maximum entries to return |
| `--status STATUS` | — | Filter by job status (`succeeded`, `failed`, `canceled`, …) |
| `--action ACTION` | — | Filter by action name (e.g. `InstallPackages`) |
| `--since DATETIME` | — | Only entries after this UTC RFC 3339 timestamp |

---

### `lacs completions <shell>`

Print a shell completion script to stdout.

```sh
lacs completions bash   >> ~/.bashrc
lacs completions zsh    >> ~/.zshrc
lacs completions fish   >> ~/.config/fish/completions/lacs.fish
```

Supported shells: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

---

### REPL (no arguments)

```sh
lacs
```

Starts an interactive session.  Each line is treated as a natural-language
intent and planned + executed in sequence.

**Key bindings:**

| Key | Action |
|---|---|
| ↑ / ↓ | Navigate command history |
| Ctrl+R | Reverse incremental history search |
| Ctrl+A / Ctrl+E | Jump to line start / end |
| Ctrl+W | Delete word before cursor |
| Ctrl+C | Cancel current line (does not exit) |
| Ctrl+D | Exit the REPL |
| `exit` / `quit` | Exit the REPL |

History is persisted to `~/.local/share/lacs/history` between sessions.

---

## Global flags

All flags apply to every subcommand and to free-form intents.

| Flag | Description |
|---|---|
| `--yes` | Auto-approve LOW-risk steps.  With `--max-risk medium`, also approves MEDIUM.  HIGH always requires human confirmation. |
| `--max-risk LEVEL` | Abort if the plan contains any step above this ceiling.  Values: `low`, `medium`, `high`. |
| `--non-interactive` | Fail immediately (`exit 3`) if any step would require interactive approval.  Use in scripts and CI. |
| `--dry-run` | Print the plan and exit without executing anything. |
| `--step-by-step` | Prompt for approval before each individual step instead of once for the whole plan. |
| `--json` | Emit NDJSON to stdout — one JSON object per event (plan, preview, result).  All colour and spinner output is suppressed.  Safe to pipe. |
| `--timeout SECS` | Hard wall-clock timeout in seconds.  Aborts the whole operation if exceeded. |
| `--log-to FILE` | Tee all stdout output to FILE in addition to the terminal.  Appends if the file exists. |

---

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Planning failed (LLM error, provider unreachable, …) |
| `2` | User rejected the plan or a step |
| `3` | Non-interactive mode but approval was required |
| `4` | Configuration or daemon error |
| `5` | Risk ceiling exceeded |
| `124` | Operation timed out (`--timeout`) |

---

## Environment variables

### LLM provider

`lacs` auto-detects the provider from API keys.  Set `LACS_LLM_PROVIDER`
to override.

| Variable | Description |
|---|---|
| `LACS_LLM_PROVIDER` | Force a provider: `anthropic`, `openai`, `gemini`, `ollama`, `groq`, `deepseek`, `mistral`, `xai` |
| `LACS_LLM_MODEL` | Override the model name for the selected provider |
| `ANTHROPIC_API_KEY` | Use the Anthropic provider (default model: `claude-sonnet-4-6`) |
| `OPENAI_API_KEY` | Use the OpenAI provider (default model: `gpt-4o-2024-11-20`) |
| `GEMINI_API_KEY` | Use the Gemini provider (default model: `gemini-2.0-flash`) |
| `LACS_ANTHROPIC_URL` | Override the Anthropic base URL (default: `https://api.anthropic.com`) |
| `LACS_OLLAMA_URL` | Override the Ollama base URL (default: `http://localhost:11434`) |
| `LACS_BRAIN_MAX_TURNS` | Planning loop turn limit — integer ≥ 1 (default: `10`) |
| `LACS_OLLAMA_THINK` | Set `true`/`false` to override thinking-mode detection for Ollama models |

**Auto-detection order** (when `LACS_LLM_PROVIDER` is not set):

1. `ANTHROPIC_API_KEY` present and non-empty → `anthropic`
2. `OPENAI_API_KEY` present → `openai`
3. `GEMINI_API_KEY` present → `gemini`
4. Otherwise → `ollama` (must be running locally)

### Daemon socket

| Variable | Description |
|---|---|
| `LACS_SOCKET` | Path to the daemon Unix socket (default: `/run/lacs/lacs.sock`) |

---

## Scripting and CI

For non-interactive use (scripts, CI pipelines), combine `--json`,
`--non-interactive`, and `--max-risk`:

```sh
# Plan only — parse the JSON to inspect before executing
PLAN=$(lacs --dry-run --json "check disk usage")
echo "$PLAN" | jq '.plan.steps[].action'

# Execute automatically up to medium risk; fail if anything higher appears
lacs --yes --max-risk medium --non-interactive "list layered packages"

# Full pipeline with a timeout and log
lacs --yes --max-risk low --non-interactive --timeout 60 \
     --log-to /var/log/lacs/run.log \
     "check disk usage"
```

The `--json` output schema:

```jsonc
// Planning output
{ "plan": { "intent": "…", "summary": "…", "steps": [
    { "action": "GetDiskUsage", "summary": "…", "risk": "low", "params": {} }
] } }

// Per-step preview (before execution)
{ "summary": "…", "risk_level": "low", "reboot_required": false,
  "warnings": [], "request_hash": "…", … }

// Per-step result (after execution)
{ "status": "succeeded", "summary": "…", "job_id": "…",
  "needs_reboot": false, "warnings": [], … }
```

---

## Examples

```sh
# Check if any services are failing
lacs "which systemd services are failed?"

# See recent LACS activity
lacs history --limit 10

# Dry-run a destructive action to inspect the plan
lacs --dry-run "layer vim via rpm-ostree"

# Execute step-by-step with manual approval of each action
lacs --step-by-step "update system"

# Non-interactive: fail fast if the plan needs a human
lacs --non-interactive --max-risk low "check memory pressure"

# Get JSON output and parse with jq
lacs --dry-run --json "list containers" | jq '.plan.steps[].action'

# Override the LLM for a single run
LACS_LLM_PROVIDER=openai OPENAI_API_KEY=sk-... lacs "check disk usage"

# Use a local Ollama model
LACS_LLM_PROVIDER=ollama LACS_LLM_MODEL=llama3.2:3b lacs "list services"
```

---

## Shell completion setup

Run once per shell:

```sh
# bash (add to ~/.bashrc)
eval "$(lacs completions bash)"

# zsh (add to ~/.zshrc)
eval "$(lacs completions zsh)"

# fish
lacs completions fish | source
```

---

## Related

- [Architecture overview](architecture.md) — trust boundary between CLI, shell, and daemon
- [Developer guide](developer-guide.md) — building and testing locally
- [User stories](user-stories.md) — end-to-end scenario descriptions
