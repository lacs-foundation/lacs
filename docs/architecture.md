# Architecture

LACS is a local Linux control plane built around a strict boundary
between planning, presentation, and execution.

## Core Components

- `lacs-brain` parses intent and produces a typed plan.
- `lacs-shell` renders the plan, preview, approval state, execution
  progress, and transaction timeline.
- `lacs-daemon` validates requests, enforces policy, executes typed
  actions, and persists transaction history.

## Trust Boundary

The daemon is trusted. The brain and shell are not trusted with raw
privileged execution.

The daemon owns:

- authorization (role-based: Observer → Dev → Admin)
- policy (stale-approval detection, request validation)
- previews (risk level, side effects, rollback metadata)
- jobs (execution, live output streaming)
- transaction records (SQLite audit log)
- rollback (automatic on failure for supported actions)

## Request Flow

1. A user enters intent in the shell.
2. The brain proposes a typed plan.
3. The shell sends each mutating step to the daemon for preview.
4. The daemon generates a preview: risk level, side effects, reboot
   requirement, rollback availability, and a content hash.
5. The shell shows the preview and captures approval.
   High-risk steps require the user to type the action name explicitly.
6. The shell sends the approved hash back to the daemon to execute.
7. The daemon verifies the hash is fresh, then runs the action.
8. During execution, the daemon streams live stdout output line-by-line
   as `JobProgress` frames.
9. The shell displays each line as it arrives.
10. On failure, if `rollback_available` is true, the daemon runs the
    rollback action automatically and reports the result.
11. The transaction is persisted to SQLite with the final job state.

## IPC Protocol

The shell and daemon communicate over a Unix domain socket
(`/tmp/lacs-daemon.sock` by default, overridable via `LACS_LISTEN_URI`).

The framing is a 4-byte little-endian `u32` length prefix followed by
a UTF-8 JSON body. Each message carries a `"type"` discriminant so
the dispatcher can route without a full decode.

Maximum message size is 4 MiB. The daemon limits concurrent
connections to 16 via a tokio semaphore; excess connections are
dropped immediately rather than queued.

The protocol is human-readable. You can inspect live traffic with:

```sh
socat - UNIX-CONNECT:/tmp/lacs-daemon.sock
```

See [ADR 0003](adr/0003-ipc-wire-protocol.md) for the rationale
behind length-prefixed JSON over gRPC or binary protobuf.

## Design Principles

- typed instead of free-form
- local instead of remote
- auditable instead of opaque
- rollback-aware instead of irreversible
- explicit approval instead of hidden mutation
