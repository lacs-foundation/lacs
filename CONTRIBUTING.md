# Contributing to LACS

Thanks for contributing. LACS is an open source project focused on
safe, auditable AI-driven Linux system management.
Changes need to stay disciplined, reviewable, and aligned with the
trust boundary.

## Before You Start

- Read the [architecture overview](docs/architecture.md).
- Read the [developer guide](docs/developer-guide.md) for prerequisites.
- Read the [ADRs](docs/adr/) for key architectural decisions.
- Open an issue before starting any substantial change.
- Keep the planner, shell, and daemon roles separate — this is the
  core invariant of the project.

## Local Development Setup

```sh
# Install pre-commit hooks (run once after cloning)
pip install pre-commit && pre-commit install

# Run all tests
cargo test --workspace
cd apps/lacs-shell && pnpm install && pnpm test

# Run hooks manually before pushing
pre-commit run --all-files
```

See [docs/developer-guide.md](docs/developer-guide.md) for full
prerequisites and run instructions.

## Issues

### Picking an issue

- Issues tagged `good first issue` are well-scoped and have clear
  acceptance criteria — ideal for a first contribution.
- Issues tagged `security` take priority over enhancements.
- Check the milestone for the current release target.
- Comment on the issue before starting work to avoid duplication.

### Opening an issue

Use one of the issue templates. Every issue should include:

- **What**: a one-sentence summary in the title
- **Why**: context and motivation
- **Where**: file paths and function names when applicable
- **Acceptance criteria**: concrete, testable conditions for done

Do not open issues for questions — use GitHub Discussions or the
`question` label.

## Pull Requests

### Branch conventions

- Use one branch (or worktree) per issue.
- Branch names: `<issue-number>-<short-slug>` (e.g. `19-role-allowlist`)
- Target `main` for all PRs.
- Keep branches small and reviewable.

### PR checklist

Before opening a PR, verify:

- [ ] `pre-commit run --all-files` passes
- [ ] `cargo test --workspace --locked` passes
- [ ] New behavior is covered by tests (write the test first)
- [ ] Docs updated if user-facing behavior changed
- [ ] PR title is one line, imperative mood (`feat:`, `fix:`, `docs:`, etc.)

### PR body template

```markdown
## Summary

One paragraph on what and why. Reference the issue: Closes #N.

## Changes

- bullet list of what changed

## Test plan

- [ ] what to verify manually
- [ ] what the automated tests cover
```

### Review process

- Every PR requires at least one review before merge.
- Address review comments with code or with a documented reason not to.
- Do not merge around an unresolved review finding.
- CI must pass before merge.

## Commit style

Follow conventional commits: `type(scope): message`

| Type | When |
| --- | --- |
| `feat` | New user-visible feature |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `chore` | Build, CI, tooling |
| `test` | Tests only |
| `refactor` | No behavior change |

Keep the subject line under 72 characters. Add a body for non-obvious
changes.

## Code standards

- Prefer explicit types and explicit error handling.
- Write the failing test first; verify it fails before writing code.
- Keep privileged behavior typed and bounded.
- Preserve approval, audit, and rollback semantics.
- Do not blur the trust boundary.

## Security

For security-sensitive issues (auth bypass, privilege escalation,
data exposure), follow the process in [SECURITY.md](SECURITY.md)
instead of opening a public issue.

## Questions

Open a GitHub Discussion or use the `question` label on an issue.
