# LACS

LACS is the Linux Agent Control Standard: a local, typed,
rollback-aware control plane for Linux systems.

It is designed for power users, system administrators, and
contributors who want to build a safe OSS platform for real system
control without giving an agent arbitrary shell access or root.

## What This Project Is

LACS is not a generic chatbot, not a browser automation framework,
and not a desktop replacement.
It is a privileged Linux execution layer with:

- typed actions
- explicit previews
- approval gating
- audit logs
- rollback metadata
- a strict boundary between planner, UI, and executor

## Current Status

The repository currently contains the design and implementation
planning docs for the v0 control plane.

- [Specification draft](docs/plans/2026-04-10-lacs-spec.md)
- [Implementation plan](docs/plans/2026-04-10-lacs-implementation-plan.md)

## Architecture at a Glance

- `zeroclaw-brain`: unprivileged planner
- `lacs-shell`: user-facing control surface
- `lacs-daemon`: trusted privileged executor

The daemon owns policy, authorization, preview generation, execution,
jobs, transactions, and rollback metadata.
The shell renders intent, preview, approval, progress, and history.
The brain proposes plans but does not mutate the system directly.

## Who Should Contribute

We welcome contributors who care about:

- Linux systems and admin workflows
- Rust and typed APIs
- Fedora Silverblue and transactional systems
- safety, auditability, and rollback
- Tauri UI work
- packaging, CI, documentation, and release engineering

## How To Get Started

1. Read the spec.
2. Read the implementation plan.
3. Open an issue for any substantial change.
4. Keep pull requests small and reviewable.
5. Preserve the trust boundary: planner, shell, and daemon stay separate.

## Contribution Standards

- Prefer small, focused pull requests.
- Document user-visible behavior.
- Add or update tests for behavior changes.
- Keep privileged operations typed and bounded.
- Preserve rollback and transaction history for every mutating action.

## Roadmap

The near-term roadmap is documented in [ROADMAP.md](ROADMAP.md).

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).
