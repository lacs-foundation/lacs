# Contributing to LACS

Thanks for contributing. LACS is intended to become a serious open
source project, so changes need to stay disciplined, reviewable, and
aligned with the system boundary.

## Before You Start

- Read the [specification draft](docs/plans/2026-04-10-lacs-spec.md).
- Read the [implementation plan](docs/plans/2026-04-10-lacs-implementation-plan.md).
- Open an issue before starting major work.
- Keep the planner, shell, and daemon roles separate.

## Good Pull Requests

- Focus on one concern per PR.
- Include tests for behavior changes.
- Update docs when behavior changes.
- Keep privileged behavior typed and bounded.
- Preserve approval, audit, and rollback semantics.

## Suggested Workflow

1. Create a branch.
2. Make the smallest useful change.
3. Run the relevant checks.
4. Open a PR with a clear summary.
5. Respond to review with code or docs updates, not narrative.

## Style

- Prefer explicit types and explicit error handling.
- Keep user-facing messages short and actionable.
- Avoid hidden behavior.
- Do not blur the trust boundary.

## Review Expectations

We will review contributions for:

- correctness
- safety
- maintainability
- test coverage
- docs quality
- alignment with the spec

## Help Wanted Areas

- Rust protocol and daemon work
- Fedora Silverblue integrations
- Tauri UI work
- CI and release engineering
- documentation
- security hardening
