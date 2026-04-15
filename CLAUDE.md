# SysKnife Operating Notes

This repository is for SysKnife, the Linux Agent Control Standard.
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
  `~/.config/superpowers/worktrees/sysknife/`.
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

- `sysknife-brain` plans.
- `sysknife-shell` presents and collects approval.
- `sysknife-daemon` executes privileged actions.
- No component should blur those roles.

## Code Quality

- No dead code. If a workaround is superseded, remove it immediately — do not
  leave it commented out or guarded by a condition that is never true.
- Do not add fallbacks, params, or flags "just in case" — every line of code
  must be reachable and load-bearing.

## Prompt Engineering — System Prompt Rules

The system prompt in `crates/sysknife-brain/src/prompt.rs` is load-bearing.
Changes to it must be validated against the full E2E story suite before merging.

### The three worked examples are not optional

`prompt.rs` contains Examples A, B, and C. **Do not remove them.**
(The original Example A — "check disk usage" — was removed; it was a strict
subset of the prose rule and the current Example A. It added no measurable
coverage. The remaining examples were renumbered B→A, C→B. Example C for
transaction history was added later — see the section below.)

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

This applies to **both** single-action and compound ("X and Y") intents.
"List containers and show services" is two read-only actions — go straight
to `propose_plan` with `ListContainers` + `ListServices`. Never query first
just because the intent mentions two things.

Use `query_*` tools ONLY when the intent is genuinely ambiguous and you need
information to DECIDE between two or more possible plans (e.g. "install vim"
→ query layered packages to check if it is already there before proposing
`AddLayeredPackage`).

### Never weaken story assertions to hide model misbehavior

E2E story assertions are the ground truth for what the model must do.
**Do not patch a failing story to accept wrong behavior.**

If the model proposes a bad plan, fix the prompt — do not broaden the
assertion to accept the bad plan as a valid alternative. Weakening an
assertion destroys its discriminating power: a test that passes for both
correct and incorrect behavior catches nothing.

Specific rule: if the model silently drops a requested action after a
query tool error, that is a model bug. Fix `prompt.rs`, not the story.

### Adding a new story or changing the prompt

Run the E2E harness against a live VM (or at minimum against the no-daemon
test CLI path) before and after. A story that passed before must not regress.

### Example C — transaction history

Example C ("did SysKnife successfully update recently?") teaches the model to use
`query_job_history` for questions about past SysKnife actions. Without it, the model
defaults to `query_deployments` or `get_system_state`, which show current system
state — not SysKnife's own transaction log.

## User Preferences — `prefs.md`

User preferences live in `~/.config/sysknife/prefs.md` and are injected into the
system prompt at the start of each `plan_intent()` call. The `remember` and
`forget` planning tools modify this file during planning.

Preferences are NOT system state. They are user-stated intentions that inform
planning decisions. Do not store system facts as preferences — those are
queryable live via `query_*` tools.

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
