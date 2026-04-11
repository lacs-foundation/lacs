# Contributing to LACS

Thanks for contributing. LACS is an open source project focused on
safe, auditable AI-driven Linux system management.
Changes need to stay disciplined, reviewable, and aligned with the
trust boundary.

## Before You Start

- Read the [architecture overview](docs/architecture.md).
- Read the [specification draft](docs/plans/2026-04-10-lacs-spec.md)
  for context on what LACS is designed to do.
- Read the [ADRs](docs/adr/) for key architectural decisions.
- Open an issue before starting any substantial change.
- Keep the planner, shell, and daemon roles separate — this is the
  core invariant of the project.

## Local Development Setup

See [docs/developer-guide.md](docs/developer-guide.md) for full
prerequisites and run instructions.

**Quick path to running tests:**

```sh
cargo test --workspace
cd apps/lacs-shell && npm install && npm test
```

`lacs-brain` works without an API key — it defaults to a local Ollama
instance. Unit and integration tests use mocks and require no network
access.

## Help Wanted

These are the highest-impact open areas:

**systemd unit and install script** — the daemon has no service file
and no install path. A sysadmin cannot deploy this without building
from source manually.

**Multi-distro action families** — every action today targets Fedora
Silverblue via `rpm-ostree`. apt (Debian/Ubuntu), dnf (Fedora
Workstation), and pacman (Arch) action families would open LACS to
the majority of the Linux user base.

**Runtime distro detection** — the daemon needs to detect the host
distro and route to the correct action family at runtime.

**Shell reconnect** — if the daemon restarts, the shell has no
recovery. Exponential-backoff reconnect with visible status is needed
for any production-ish use.

**config.toml** — LLM provider, socket path, and model name should be
persistable to `~/.config/lacs/config.toml` instead of requiring
environment variables every session.

**Security hardening** — the role-to-action authorization is correct,
but there is no rate limiting, no per-user policy file, and no audit
alerting.

**Documentation** — architecture explanations, contributor tutorials,
and real-world usage examples all help lower the barrier to entry.

## Good Pull Requests

- Focus on one concern per PR.
- Write the failing test first; verify it fails before writing code.
- Include tests for every behavior change.
- Update docs when behavior changes.
- Keep privileged behavior typed and bounded.
- Preserve approval, audit, and rollback semantics.

## Suggested Workflow

1. Open or claim an issue.
2. Create a branch (or worktree for larger changes).
3. Make the smallest useful change.
4. Run the CI checks locally.
5. Open a PR with a clear summary.
6. Respond to review feedback with code or doc updates.

## Style

- Prefer explicit types and explicit error handling.
- Keep user-facing messages short and actionable.
- Avoid hidden behavior.
- Do not blur the trust boundary.

## Review Expectations

We review contributions for:

- correctness and safety
- test coverage
- documentation quality
- maintainability
- alignment with the trust model and architecture

## Questions

Open a GitHub issue with the `question` label.
For security-sensitive issues, follow the process in
[SECURITY.md](SECURITY.md) instead.
