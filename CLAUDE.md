# LACS Operating Notes

This repository is for LACS, the Linux Agent Control Standard.
Work here must preserve the trust boundary between the planner,
the shell, and the privileged daemon.

## Repository Workflow

- Use isolated git worktrees for feature work.
- Prefer one branch per task or tightly related task batch.
- Keep branches small, reviewable, and focused on a single concern.
- Use the PR template for every pull request.
- Open PRs only against `main`.
- Request a code review before merge.
- Apply review fixes, commit them, and push the branch again.
- Merge only after the branch is reviewed and the checks pass.
- Delete the remote branch after merge.
- Delete the local branch after merge.
- Delete the worktree directory after merge.

## Worktree Convention

- Keep worktrees outside the repo under
  `~/.config/superpowers/worktrees/lacs/`.
- Do not leave merged worktrees around.
- Clean up the worktree during branch completion, not later.

## Implementation Standards

- Use test-driven development for behavior changes.
- Write the failing test first.
- Verify the failure before writing implementation code.
- Keep tests close to exhaustive for protocol, daemon, and safety behavior.
- Prefer deterministic unit and integration tests.
- Run the relevant checks frequently instead of waiting until the end.
- Treat regressions as blockers, not polish items.

## CI Expectations

- Keep CI strict enough to catch formatting, lint, and test
  regressions before merge.
- Prefer locked dependency resolution for Rust builds and tests.
- Keep contributor-facing docs lint-clean.
- Keep workflow YAML lint-clean.

## Code Review Expectations

- Review every branch before merge.
- Treat review findings as actionable engineering feedback.
- Fix correctness, safety, and coverage issues before merging.
- Do not merge around an unresolved review finding unless it has
  been proven incorrect.

## Project Boundaries

- `lacs-brain` plans.
- `lacs-shell` presents and collects approval.
- `lacs-daemon` executes privileged actions.
- No component should blur those roles.

## Code Quality

- No dead code. If a workaround is superseded, remove it immediately — do not
  leave it commented out or guarded by a condition that is never true.
- Do not add fallbacks, params, or flags "just in case" — every line of code
  must be reachable and load-bearing.

## Prompt Engineering — System Prompt Rules

The system prompt in `crates/lacs-brain/src/prompt.rs` is load-bearing.
Changes to it must be validated against the full E2E story suite before merging.

### The two worked examples are not optional

`prompt.rs` contains Examples A and B. **Do not remove them.**
(The original Example A — "check disk usage" — was removed; it was a strict
subset of the prose rule and the current Example A. It added no measurable
coverage. The remaining examples were renumbered B→A, C→B.)

Empirical result (GPT-4o, 7 read-only stories, 2026-04-14):

| Condition           | Read-only stories passing |
|---------------------|--------------------------|
| With examples (A+B) | 7 / 7                    |
| Without examples    | 3 / 7                    |

Stories 8–10 require a live daemon (rpm-ostree, toolbox, SSH key writes) and are
skipped in the no-daemon CI run. True pass rate is 7/7 read-only + 0/3 destructive
(daemon absent); the destructive stories pass on the real VM.

Without the examples, GPT-4o defaults to always querying state first
(`get_system_state` or `query_*` tools) before proposing any plan. This is
wrong for direct read-only requests and causes two failure modes:

1. **Hard crash** — `get_system_state` failure propagates immediately via `?`
   in `planner.rs`; planning returns `StateUnavailable` with no plan.
2. **Wrong plan** — `query_*` tool errors are returned as tool results; the
   model receives an error, falls into a recovery path, and proposes an
   unrelated action (`CollectDiagnostics` for a memory query, `GetDiskUsage`
   for a firewall query, etc.).

### The rule the examples encode

> **Direct read-only request → skip all query tools → call `propose_plan`
> immediately with the matching `Get*` / `List*` action.**

Use `query_*` tools ONLY when the intent is genuinely ambiguous and you need
information to DECIDE between two or more possible plans (e.g. "install vim"
→ query layered packages to check if it is already there before proposing
`AddLayeredPackage`).

### Adding a new story or changing the prompt

Run the E2E harness against a live VM (or at minimum against the no-daemon
test CLI path) before and after. A story that passed before must not regress.

## OpenAI Responses API — Dual-ID Protocol

The OpenAI Responses API uses two distinct identifiers per tool call:

| Field     | Format     | Purpose                                          |
|-----------|------------|--------------------------------------------------|
| `id`      | `fc_xxx`   | Response-item ID; must be echoed verbatim in the next input array as the assistant's function_call item ID |
| `call_id` | `call_xxx` | Function-call match key; must appear as `call_id` on the corresponding `function_call_output` item |

`ContentBlock::ToolUse` therefore carries **both** fields. `ToolResultBlock` mirrors
`call_id` so the adapter can set `ToolResult.call_id = call_xxx` correctly.

Do NOT collapse the two into a single "effective_id". Providers that have no
separate `call_id` (Anthropic, Ollama, Gemini) leave it as `None` and the
adapter falls back to `id` — the fallback must stay invisible to callers.
