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
