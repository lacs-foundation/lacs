# Developer Guide

This guide explains how to work on LACS as a contributor.

## Read First

- [Specification draft](plans/2026-04-10-lacs-spec.md)
- [Implementation plan](plans/2026-04-10-lacs-implementation-plan.md)
- [Architecture overview](architecture.md)

## Working Style

LACS is designed as a large open source project.

That means:

- keep changes small
- keep behavior typed and explicit
- keep the daemon as the only executor
- update docs when behavior changes
- add tests for every new rule

## Repository Layout

- `IMPLEMENTATION_LACS.md` is the original design memo.
- `docs/plans/` contains the live spec and implementation plan.
- future code will live under `crates/` and `apps/`.

## Contribution Workflow

1. Open or find an issue.
2. Align the change with the spec.
3. Make the smallest implementation that solves the problem.
4. Add or update tests.
5. Update docs if user-facing behavior changed.
6. Submit a focused PR.

## Quality Bar

Before merging, a change should be:

- understandable
- testable
- documented
- safe by default
- consistent with the trust model
