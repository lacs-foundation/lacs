# LACS — MVP Gaps, Remaining Work, and Growth Strategy

> **Not tracked by git.** Lives in `docs/local/` which is gitignored.
> Last updated: 2026-04-10.

---

## Current state in one sentence

The architecture is clean and well-tested (176 passing tests). The
brain plans. The daemon can preview, execute, and record. But the
socket connection between shell and daemon is not wired, so the product
does nothing end-to-end on a real system.

---

## Part 1 — What is not wired yet

### 1.1 The four remaining IPC steps

The daemon IPC spec (`docs/plans/2026-04-10-lacs-daemon-ipc-spec.md`)
tracks implementation as an 8-step sequence. Steps 1–4 are done.
Steps 5–8 remain.

| Step | File to create/modify | What it does |
|---|---|---|
| 5 | `crates/lacs-daemon/src/dispatcher.rs` | `connection_handler`: reads a `DaemonRequest` from a framed stream, authenticates the caller via group membership, routes to preview or execute, streams `job_event` messages back |
| 6 | `crates/lacs-daemon/src/main.rs` | Accept loop: `listener.accept()` → spawn Tokio task → `connection_handler` |
| 7 | `apps/lacs-shell/src-tauri/src/daemon_client.rs` | `DaemonIpcClient`: opens the Unix socket, implements `StateClient` (replaces `DemoStateClient`), sends preview/execute/cancel requests, runs a reader task for streaming events |
| 8 | `apps/lacs-shell/src-tauri/src/commands.rs` | Wire `approve_preview` to daemon; replace `DemoStateClient`; surface `DaemonIpcClient` errors as typed `ShellError` |

Everything the dispatcher needs already exists:
- `FramedStream` (framing.rs) — done
- `build_action_spec` + `execute_spec` (executor.rs) — done
- `preview_action` (preview.rs) — done
- `collect_state` (state_collector.rs) — done
- `TransactionStore` (transactions.rs) — done, including `update_status`
- `highest_role_from_groups` (auth.rs) — done
- `require_fresh_approval` (policy.rs) — done

The dispatcher is purely wiring, not invention.

### 1.2 Policy: role-to-action allowlist is missing

`policy.rs` today only validates the approval hash. There is no
action-to-role allowlist: no code checks whether a `Dev` is allowed
to call `DeleteUser` or whether an `Observer` can preview `UpdateSystem`.

The dispatcher needs to enforce this before routing to preview or
execute. Without it, the CallerRole tiers are cosmetic.

Suggested minimum: a `fn action_requires_role(action_name: &str) ->
CallerRole` function that returns the minimum role required, and a
policy check in the dispatcher that returns `authorization_failure` if
the caller's role is below that threshold.

### 1.3 The UX plan (15 tasks)

Separately tracked in `docs/superpowers/plans/2026-04-10-ux-error-flows.md`.
Covers: typed `ShellError`, risk-scaled approval gate, `ErrorBlock`
component, `PlanPane` step breakdown, state-driven CSS grid,
`ExecutionPane` with cancel, timestamped timeline. None of these tasks
have been started yet.

---

## Part 2 — What is needed for a mature MVP

These gaps do not block compilation but block real-world use.

### 2.1 Privilege architecture and packaging

The daemon must run as root to execute `rpm-ostree`, `systemctl`,
`useradd`, and similar privileged commands. Currently there is no:

- systemd service unit file (`lacs-daemon.service` with `User=root`)
- polkit rules or setuid wrapper
- RPM, Flatpak, or AppImage package for the shell
- Install script, Makefile target, or `cargo install` path

A sysadmin cannot install or run this without building from source and
manually starting the daemon as root. This blocks every user who is
not also a Rust developer.

**What to build:**
- `packaging/lacs-daemon.service` — systemd unit, `User=root`,
  `ExecStart=/usr/local/bin/lacs-daemon`, `Restart=on-failure`
- `Makefile` with `install` target: copies daemon binary to
  `/usr/local/bin`, enables and starts the service
- Tauri `tauri.conf.json` bundle config for AppImage (Linux, easiest
  cross-distro) and RPM (Fedora-native)
- README quick-start section: 3 commands from zero to running

### 2.2 Cross-distro action coverage

Every action family today targets Fedora Silverblue/Kinoite via
`rpm-ostree`. That is approximately 50,000 active installs globally —
a niche within a niche. For broad adoption the daemon needs action
families for:

| Distro family | Package manager | Most important actions |
|---|---|---|
| Debian / Ubuntu / Mint | `apt` | install, remove, update, autoremove |
| Fedora Workstation (non-ostree) | `dnf` | install, remove, upgrade |
| Arch / Manjaro | `pacman` | -S, -R, -Syu, AUR helper |

The `build_action_spec` + `execute_spec` architecture already supports
this — it is additive work, not structural change. The daemon also
needs runtime distro detection so it routes to the right action family
for the host.

Without multi-distro support, the addressable install base for LACS is
too small to generate organic growth.

### 2.3 Rollback is metadata, not execution

`rollback_available: true` appears in preview profiles for High-risk
actions (UpdateSystem, RemovePackages, RebaseSystem, etc.). But there
is no code path that *runs* a rollback when `execute_spec` fails.

When an action fails today, the job is marked `Failed`. The
`RolledBack` job state exists in the state machine but is never
reached by any code path.

**What to build:**
- After `execute_spec` returns `Err`, check `spec.rollback_available`
- If true, look up the rollback action for that action family (e.g.,
  `rpm-ostree rollback` for deployment actions)
- Execute the rollback spec; on success, transition job to `RolledBack`
- Record the rollback transaction
- Emit a `job_event` with the rollback result

This is the feature that makes the safety narrative real. "The AI
agent that can undo itself" only works if the rollback actually runs.

### 2.4 Job event streaming is undesigned on the daemon side

The IPC spec defines `job_event` messages (`step_started`,
`step_output`, `step_completed`) but the daemon does not emit them yet.
During a long `rpm-ostree update` (2–3 minutes), the shell would show
nothing. For any slow operation this kills the perceived reliability.

The dispatcher needs to spawn the execution in a task and stream
`job_event` frames back on the same connection as the job runs. The
shell's reader task (part of `DaemonIpcClient`) needs to dispatch these
as `timeline_event` reducer actions.

### 2.5 Reconnect logic

If the daemon crashes or the socket path disappears mid-session, the
shell has no recovery. For a tool used on servers this needs:
- Detection of a broken connection (EOF or IO error on the socket)
- Exponential backoff reconnect attempts (e.g., 1s, 2s, 4s, max 30s)
- Update `daemonStatus` to `"unreachable"` during reconnect window
- Auto-transition back to `"connected"` on successful reconnect
- Surface the reconnect state visibly in the shell chrome

### 2.6 First-run and configuration experience

- `ANTHROPIC_API_KEY` is required for the Anthropic provider but there
  is no guided setup. First run with a missing key produces a Rust
  panic or a cryptic error.
- Ollama fallback works in `config.rs` but is not documented as the
  recommended default for users who don't have an API key.
- No config file support — everything is environment variables. For a
  desktop app, a `~/.config/lacs/config.toml` would be more ergonomic.

---

## Part 3 — The 70,000 GitHub stars question

### Honest baseline

Open Interpreter — the closest project in spirit — reached ~56k stars
but not in 2 months. Shell-GPT is at ~25k. Lazydocker reached ~16k
stars in its first month (it went viral on HN). Projects in this space
that hit 70k in 2 months are statistical outliers requiring a viral
moment that is hard to engineer.

What *can* be engineered: the conditions that make a viral moment
possible. Here is what those conditions look like for LACS.

### The narrative that could make it viral

LACS has a genuinely differentiated position that no other project
owns clearly:

> "The only AI agent for Linux that cannot do anything you didn't
> explicitly review and approve — with a full audit trail, typed
> actions, and rollback."

The comparison writes itself:

| Tool | Problem LACS solves |
|---|---|
| Open Interpreter | Runs arbitrary shell commands with no friction. No audit log. No rollback. Dangerous on servers. |
| Claude Computer Use | Uncontrolled. Terrifying on production infrastructure. |
| Ansible | Requires YAML playbooks, inventory files, roles, and knowing what you want to do before you start. |
| Manual commands | No audit trail. No rollback. Easy to typo a destructive command. |

The target audience — sysadmins, DevOps, Fedora power users — is
exactly the audience that would never touch Open Interpreter on a
production machine but *would* use something with an explicit approval
gate and an audit log. LACS is the responsible, conservative option.
That is rare in AI tooling and genuinely sellable.

### The five things that determine whether it goes viral

**1. It must actually work before any public announcement.**
A demo that uses `DemoStateClient` with hardcoded Silverblue fixtures
will be spotted immediately by anyone who reads the source. The IPC
wiring, rollback, and packaging must ship before launch. A demo on a
real Silverblue machine doing a real `rpm-ostree update` with a visible
rollback is the only credible demo.

**2. The README must be user-first, not contributor-first.**
The current README opens with architecture boundaries and contributor
guidelines. A user landing from HN wants to know: what does this do,
why should I care, how do I install it, what does it look like. The
architecture section should be below the fold.

Required in the README before launch:
- Animated GIF or video embed showing the full flow: type intent →
  see plan with risk badges → type action name to confirm high-risk
  step → watch execution with live timeline → see rollback on failure
- One-paragraph value proposition at the top
- "Install in 3 commands" section as the first technical content
- "vs alternatives" comparison table
- Clear Ollama path (no API key required)

**3. Ollama / local-first must be the documented default.**
Sysadmins do not put `ANTHROPIC_API_KEY` on production servers.
Privacy-conscious users will not use a cloud LLM for system management.
Ollama + Llama 3 (or similar) as the zero-configuration default is a
major selling point and should be the first path in the README, not a
footnote.

**4. Multi-distro coverage must ship at launch or very shortly after.**
Launching as "works on Fedora Silverblue" limits the audience to a
tiny slice of the Linux world. Ubuntu users are the largest single
Linux demographic on GitHub. The `apt` action family should ship at
launch or within the first week. Arch support (pacman) brings the
enthusiast crowd who amplify things on Reddit and HN.

**5. The launch post matters as much as the product.**
HN "Show HN" posts that reach the front page can deliver 2,000–5,000
stars in a single day. The post needs:
- First sentence: "LACS is an AI agent for Linux that shows you a
  plan and asks for your approval before changing anything."
- Link to the GIF/video demo — text alone does not move people
- Clear distinction from Open Interpreter (the comparison people
  will immediately make)
- Call to action for contributors (the Rust + Linux community
  produces active contributors, not just star-givers)

Amplification channels in order of expected impact:
1. Hacker News "Show HN"
2. r/linux (large, shares this type of tool)
3. r/rust (amplifies well-written Rust tools)
4. r/selfhosted (sysadmin audience, exactly the target user)
5. r/devops
6. YouTube (a 5-minute demo from a Linux YouTuber like Chris Titus
   Tech or DistroTube could deliver thousands of stars on its own)

### Sequenced plan to get there

```
Phase 1 — Make it work (no public announcement until this is done)
  IPC steps 5–8 (dispatcher + shell client)     ~1 week
  UX plan (15 tasks)                             ~1 week
  Policy role-to-action allowlist                ~2 days
  Rollback execution path                        ~3 days
  Privilege packaging (systemd unit, install)    ~3 days
  Job event streaming                            ~3 days

Phase 2 — Make it accessible
  apt action family (Debian/Ubuntu)              ~3 days
  dnf action family (Fedora Workstation)         ~2 days
  pacman action family (Arch)                    ~2 days
  Ollama as default, zero-config local setup     ~1 day
  config.toml support                            ~2 days
  Install in 3 commands story                    ~1 day

Phase 3 — Make it compelling
  Real demo on real Silverblue hardware          ~1 day
  Animated GIF of approval gate + rollback       ~half day
  README rewrite (user-first)                    ~half day
  vs-alternatives comparison table               ~half day
  Ollama quick-start section                     ~half day

Phase 4 — Launch
  Show HN post
  r/linux, r/rust, r/selfhosted, r/devops
  Reach out to 2–3 Linux YouTubers with the demo video
```

### What a realistic outcome looks like

If Phase 1–3 execute cleanly and the HN post lands:

- **Realistic:** 5,000–15,000 stars in the first month
- **Strong viral moment:** 20,000–40,000 in the first month
- **70,000 in 2 months:** requires a second viral moment (YouTuber,
  major tech publication, prominent retweet) on top of a strong launch

The honest answer is: 70k in 2 months is possible but not engineerable
on its own. What *is* engineerable is a product that deserves it and a
launch that maximizes the probability. The architecture already
deserves it. The product is not there yet.
