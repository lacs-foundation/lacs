# Roadmap

This roadmap is intentionally high level. It is meant to show
contributors where the project is going and what matters next.

## Phase 1: Foundation

- ~~publish the OSS project files~~ — done
- ~~keep the spec and implementation plan current~~ — done
- ~~establish CI, linting, and repository hygiene~~ — done
- ~~keep the architecture boundary explicit~~ — done

## Phase 2: Protocol and Daemon

- ~~implement the shared protocol crate~~ — done; `lacs-proto` and `lacs-types`
- ~~implement the privileged daemon skeleton~~ — done; action families, policy,
  auth, preview, jobs, transactions
- ~~persist transactions and approvals~~ — done; SQLite-backed `TransactionStore`
  with full CRUD and `update_status`
- ~~generate previews and job state~~ — done; `preview_action` covers all action
  families
- ~~IPC framing~~ — done; `FramedStream` with 4-byte LE length-prefix framing
- ~~state collection~~ — done; `collect_state` via `CommandRunner` trait
- ~~action executor~~ — done; `build_action_spec` + `execute_spec` for all ~60
  actions
- ~~wire the dispatcher~~ — done; `connection_handler` with role-based auth,
  preview, execute, and live streaming
- ~~wire the accept loop~~ — done; tokio accept loop with 16-connection limit
  and graceful shutdown

## Phase 3: Core Actions

- ~~deployment and boot controls~~ — done
- ~~Flatpak app lifecycle~~ — done
- ~~toolbox workflows~~ — done
- ~~layered package management~~ — done
- ~~package repository management~~ — done
- ~~container and runtime management~~ — done
- ~~services, network, identity, and user management~~ — done

## Phase 4: Brain and Shell

- ~~implement the planner runtime~~ — done; `lacs-brain` has Anthropic and Ollama
  providers, a tool-use loop, plan validation, and risk classification
- ~~implement the shell UI~~ — done; intent, plan, approval gate, job timeline,
  and error states wired end-to-end
- ~~wire previews~~ — done; daemon preview handler and shell preview renderer both
  complete
- ~~wire approvals, jobs, and timeline to the daemon dispatcher~~ — done;
  `approve_preview` routes through the daemon, job progress streamed live
- ~~replace `DemoStateClient` with real daemon IPC~~ — done; `DaemonIpcClient`
  with 600-second execute timeout and live progress frames
- ~~live stdout streaming~~ — done; each output line sent as a `JobProgress`
  frame as the process runs
- ~~automatic rollback~~ — done; High-risk rpm-ostree failures trigger
  `rpm-ostree rollback` automatically

## Phase 5: Release Quality

The current focus. Items are roughly in priority order.

- systemd unit file (`lacs-daemon.service`) and install script
- multi-distro action families: apt (Debian/Ubuntu), dnf (Fedora Workstation),
  pacman (Arch)
- runtime distro detection so the daemon routes to the correct action family
- shell reconnect with exponential backoff when the daemon socket disappears
- `~/.config/lacs/config.toml` support for persistent LLM and socket settings
- Tauri bundle configuration for AppImage and RPM
- harden the test matrix (integration tests against a real daemon socket)
- stabilize the wire protocol and cut a v0.1 release
- contributor-facing demo on real hardware with rollback visible
