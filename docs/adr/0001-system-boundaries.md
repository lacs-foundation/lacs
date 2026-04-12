# ADR 0001: System Boundaries

## Status

Accepted.

## Context

LACS exists to let an agent perform real Linux administration without
granting it arbitrary shell access or root.

## Decision

LACS uses three separate roles:

- `lacs-brain` plans
- `lacs-shell` presents and collects approval
- `lacs-daemon` executes

The daemon is the only privileged executor. The brain is never
allowed to mutate the system directly.

## Consequences

- The trust boundary is simple and auditable.
- The shell remains a client instead of a second privileged runtime.
- The daemon can enforce policy, preview, transaction logging, and
  rollback in one place.
- The project can grow without collapsing into an unsafe command runner.
