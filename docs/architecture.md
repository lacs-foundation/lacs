# Architecture

LACS is a local Linux control plane built around a strict boundary
between planning, presentation, and execution.

## Core Components

- `zeroclaw-brain` parses intent and produces a plan.
- `lacs-shell` renders the plan, preview, approval state, execution
  progress, and transaction timeline.
- `lacs-daemon` validates requests, enforces policy, executes typed
  actions, and persists transaction history.

## Trust Boundary

The daemon is trusted. The brain and shell are not trusted with raw
privileged execution.

The daemon owns:

- authorization
- policy
- previews
- jobs
- transaction records
- rollback hints

## Request Flow

1. A user enters intent in the shell.
2. The brain proposes a typed plan.
3. The daemon generates previews for mutating actions.
4. The shell shows the preview and captures approval.
5. The daemon executes the action as a job.
6. The shell streams progress and records the transaction.

## Design Principles

- typed instead of free-form
- local instead of remote
- auditable instead of opaque
- rollback-aware instead of irreversible
- explicit approval instead of hidden mutation
