# Roadmap

This roadmap is intentionally high level. It is meant to show
contributors where the project is going and what matters next.

## Phase 1: Foundation

- publish the OSS project files
- keep the spec and implementation plan current
- establish CI, linting, and repository hygiene
- keep the architecture boundary explicit

## Phase 2: Protocol and Daemon

- ~~implement the shared protocol crate~~ — done; `lacs-proto` and `lacs-types`
- ~~implement the privileged daemon skeleton~~ — done; action families, policy,
  auth, preview, jobs, transactions
- ~~persist transactions and approvals~~ — done; SQLite-backed `TransactionStore`
  with full CRUD and `update_status`
- ~~generate previews and job state~~ — done; `preview_action` covers all action families
- ~~IPC framing~~ — done; `FramedStream` with 4-byte LE length-prefix framing
- ~~state collection~~ — done; `collect_state` via `CommandRunner` trait
- ~~action executor~~ — done; `build_action_spec` + `execute_spec` for all ~60 actions
- wire the dispatcher — `connection_handler` connecting framing → auth → preview/execute loop
- wire the accept loop in `main.rs`

## Phase 3: Core Actions

- ~~deployment and boot controls~~ — done
- ~~Flatpak app lifecycle~~ — done
- ~~toolbox workflows~~ — done
- ~~layered package management~~ — done
- ~~package repository management~~ — done
- ~~container and runtime management~~ — done
- ~~services, network, identity, and user management~~ — done

## Phase 4: Brain and Shell

- ~~implement the planner runtime~~ — done; `lacs-brain` has Anthropic
  and Ollama providers, a tool-use loop, plan validation, and risk
  classification
- implement the sparse shell UI
- ~~wire previews~~ — done on the brain side; daemon-side preview handler pending
- wire approvals, jobs, and timeline to the daemon dispatcher
- replace `DemoStateClient` with real daemon IPC (`DaemonIpcClient`)

## Phase 5: Release Quality

- harden the test matrix
- improve packaging
- improve docs for contributors and users
- cut public releases
- stabilize the API surface
