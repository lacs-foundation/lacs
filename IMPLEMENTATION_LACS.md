# IMPLEMENTATION.md — LACS MVP on Fedora Silverblue (ZeroClaw Brain)

**Project:** Linux Agent Control Standard (LACS)  
**MVP target:** General desktop on **Fedora Silverblue** (rpm-ostree)  
**Brain:** **ZeroClaw** (unprivileged, Rust)  
**Control plane:** `lacs-daemon` (privileged, Rust)  
**Shell:** `lacs-shell` (Tauri full-screen session)  

---

## 0. What we are building

A desktop that *feels* like an “AI OS” while remaining a normal Linux system. The key is a hardened, typed control plane.

**Invariant:** the brain never gets `sudo` and never executes arbitrary shell. It only proposes and calls **typed LACS APIs**.  
**Invariant:** all high-impact changes are **transactional**, **audited**, **previewed**, and **rollbackable**.

---

## 1. Architecture

### 1.1 Components

1) **lacs-daemon** (root)  
- Exposes a **local-only** gRPC API on a Unix domain socket: `/run/lacs/lacs.sock`  
- Implements policy, risk classification, allowlists, validation, redaction, audit, and adapters  
- Executes the OS actions safely using `std::process::Command` (no shell), with timeouts and output capture.

2) **lacs-shell** (user session)  
- Full-screen shell (Tauri) that replaces GNOME Shell as the default session (MVP)  
- UI responsibilities:
  - chat/intent entry
  - plan preview (computed risks, reboot requirement, diffs)
  - approval capture
  - execution progress
  - timeline/audit viewer
  - rollback helper (staging rollback + reboot prompts)

3) **zeroclaw-brain** (user session or `systemd --user`)  
- Persistent agent runtime (Rust) for:
  - intent parsing
  - planning
  - calling read-only LACS APIs
  - generating a **Plan** object (list of typed actions)
- By default it does **not** execute mutating actions; the UI executes them after approval (recommended).

### 1.2 Trust boundaries

- **Untrusted:** LLM/provider outputs, web content, user documents, terminal text, external pages.
- **Trusted:** `lacs-daemon` code, its policy tables, its validation/redaction.
- **Semi-trusted:** `lacs-shell` (should not have root; only requests).
- **Credential boundary:** secrets never pass through the brain; only through daemon’s secure store using `credential_ref`.

### 1.3 Data flow (recommended)

1. User types in `lacs-shell`.
2. Shell sends message to `zeroclaw-brain`.
3. Brain calls read-only APIs (system state, diagnostics) if needed.
4. Brain returns:
   - natural language explanation
   - a **Plan**: ordered list of LACS actions
5. Shell requests a **PolicyPreview** from daemon (risk levels, reboot flag, diffs).
6. User approves required actions.
7. Shell executes actions by calling daemon.
8. Shell renders results + adds to timeline view.

---

## 2. Why Silverblue (rpm-ostree) for MVP

Silverblue provides **transactional OS state**:
- Updates create a new deployment; rollback is selecting prior deployment.
- `/usr` is immutable and versioned; drift is minimized.
- This makes “agent does system changes” tractable because changes are naturally **commit-like**.

**Desktop UX:** apps are installed via Flatpak without reboot; only base-layer changes require staging/reboot.

---

## 3. MVP capability set (12 typed APIs)

The MVP must implement exactly these 12 actions (see `SPEC.md` for full schemas):

**Read-only**
1. `GetSystemState`
2. `CollectDiagnostics`

**Flatpak apps**
3. `InstallFlatpak`
4. `RemoveFlatpak`

**rpm-ostree base**
5. `InstallPackages`
6. `RemovePackages`
7. `UpdateSystem`

**systemd**
8. `RestartService`
9. `SetServiceEnabled`

**NetworkManager**
10. `ConfigureWifi`
11. `SetDnsServers`

**Rollback**
12. `RollbackDeployment`

---

## 4. Policy & approvals (MVP)

### 4.1 Risk levels

- **LOW:** no system mutation or safe read operations  
  e.g., `GetSystemState`, `CollectDiagnostics`
- **MEDIUM:** reversible or localized mutation  
  e.g., `InstallFlatpak`, `RestartService`, `SetDnsServers`, `ConfigureWifi`
- **HIGH:** deployment-level mutation / reboot path  
  e.g., `UpdateSystem`, `InstallPackages`, `RemovePackages`, `RollbackDeployment`

### 4.2 Approval rules

- LOW: no approval
- MEDIUM: single user confirmation in UI (plan preview)
- HIGH: explicit confirmation + “reboot required” indicator; optional re-auth hook (future)

### 4.3 Allowlists (MVP)

- Flatpak remote allowlist: `flathub` only (configurable)
- systemd unit allowlist: start minimal  
  - `NetworkManager.service`, `bluetooth.service`, `cups.service` (optional `docker.service`)
- rpm-ostree package allowlist/denylist:
  - MVP uses denylist for “dangerous” low-level packages and anything that expands remote access unexpectedly.
  - Prefer an allowlist later (enterprise mode).

### 4.4 Rate limiting (MVP)

To avoid DoS loops from the brain:
- `RestartService`: max 3/min per service
- `ConfigureWifi`: max 5 attempts/10 min
- `UpdateSystem` and `RollbackDeployment`: max 1/30 min

---

## 5. Transaction model & audit timeline

### 5.1 Transaction record

Every mutating API produces a `Transaction` persisted in SQLite:

- `tx_id` (UUIDv7)
- timestamp (UTC)
- actor:
  - UI user
  - brain instance id (if any)
- requested action + parameters (redacted)
- policy decision:
  - risk level
  - approval id
- pre-state summary (selected fields hash)
- steps executed (adapter calls)
- outputs (redacted, bounded)
- post-check results
- rollback hints (deployment id, etc.)
- final status

### 5.2 Snapshot/rollback strategy

- **System deployments**: rpm-ostree provides rollback.  
  - `UpdateSystem` stages a new deployment.
  - `RollbackDeployment` stages rollback to previous.
- **Config changes**: the MVP avoids arbitrary `/etc` edits; network changes are done via NetworkManager, which is itself recoverable.  
  - If you later add `ModifyConfig`, you must implement `/etc` file snapshots under `/var/lib/lacs/etc-snapshots/<tx_id>/`.

### 5.3 Plan preview

Before executing a plan, the shell requests `PolicyPreview` (internal helper, not counted in the 12 public APIs unless you want to expose it). The daemon returns:

- per-action risk
- reboot required
- expected side effects
- services touched
- diff summaries (where applicable)
- estimated duration class (fast/slow)

---

## 6. Security design (MVP baseline)

### 6.1 Prompt injection mitigation

Typed APIs mitigate injection by removing the universal actuator (arbitrary shell/root). The daemon enforces:

- strict schemas
- allowlists
- denylists
- no secret reads
- no arbitrary file I/O
- no arbitrary network egress for the daemon

### 6.2 Credential handling

- The UI collects secrets and passes to daemon once.
- Daemon stores secrets behind `credential_ref` and TTL.
- Brain only ever receives and uses `credential_ref`.
- Diagnostics outputs are redacted; secrets are never returned.

### 6.3 Local-only attack surface

- gRPC exposed only on Unix socket `/run/lacs/lacs.sock`
- socket permissions: `root:lacs` 0660
- UI user is in `lacs` group
- Brain process is **not** in `lacs` group (recommended)
  - Brain communicates to UI; UI executes mutations (strong safety)

### 6.4 Execution safety

- Never invoke `/bin/sh -c ...`
- Use `Command` with fixed argument lists
- Hard timeouts per adapter call
- Bounded stdout/stderr capture
- Redaction before persistence or returning to UI/brain

---

## 7. Implementation details

### 7.1 Rust crates

- `lacs-proto`: protobuf definitions
- `lacs-daemon`:
  - gRPC server (tonic)
  - policy engine
  - adapters (ostree, flatpak, systemd, nm, journal)
  - transaction/audit store
  - credential store
- `lacs-policy`: tables, allowlists, risk rules
- `lacs-redact`: normalization + redaction library
- `lacs-tx`: transaction + SQLite repository

### 7.2 IPC and transport

- gRPC over UDS for daemon API
- UI ↔ Brain:
  - local HTTP server in brain: `127.0.0.1:8787`
  - endpoints:
    - `POST /chat` -> `{ text, plan? }`
    - `POST /plan` -> `{ plan }`

### 7.3 Adapters

**ostree adapter**
- `rpm-ostree status --json`
- `rpm-ostree upgrade`
- `rpm-ostree install <pkgs...>`
- `rpm-ostree uninstall <pkgs...>`
- `rpm-ostree rollback`

**flatpak adapter**
- `flatpak install -y <remote> <app_id>`
- `flatpak uninstall -y <app_id>`
- `flatpak info --show-permissions <app_id>`
- `flatpak list --app --columns=application,version,origin`

**systemd adapter**
- `systemctl is-active <unit>`
- `systemctl restart <unit>`
- `systemctl enable --now <unit>`
- `systemctl disable --now <unit>`

**NetworkManager adapter**
- `nmcli -t -f DEVICE,TYPE,STATE dev status`
- `nmcli dev wifi list` (optional)
- `nmcli dev wifi connect <ssid> password-file <tmp>`
- `nmcli con show --active` (to select target connection)
- `nmcli con mod <conn> ipv4.dns ...` and `nmcli con up <conn>`

**journald adapter**
- `journalctl -b --priority=err --since ...`
- `journalctl -u NetworkManager --since ...`
- `journalctl -u <unit> --since ...`

All outputs are redacted, bounded, and structured.

---

## 8. System integration (units, permissions, session)

### 8.1 systemd units (daemon)

Create:

- `/etc/systemd/system/lacs-daemon.service`
- `/etc/systemd/system/lacs-daemon.socket`

Socket activation preferred.

`lacs-daemon.socket`:
- `ListenStream=/run/lacs/lacs.sock`
- `SocketMode=0660`
- `SocketUser=root`
- `SocketGroup=lacs`

`lacs-daemon.service`:
- `User=root`
- `Group=root`
- `ExecStart=/usr/local/bin/lacs-daemon`
- `NoNewPrivileges=true`
- `PrivateTmp=true`
- `ProtectSystem=strict` (ensure it still can call required binaries; tune)
- `ProtectHome=true` (daemon should not read home)
- `CapabilityBoundingSet=` minimal (ideally empty; root with filesystem restrictions)
- `RestrictAddressFamilies=AF_UNIX AF_NETLINK` (tune for nmcli/systemd)
- `SystemCallFilter=` (future hardening)

### 8.2 User services (brain)

`~/.config/systemd/user/zeroclaw-brain.service`:
- Runs brain server on localhost
- Stores memory in user directory, not system
- No access to `/run/lacs/lacs.sock` (not in group)

### 8.3 Desktop session (shell)

Provide a session entry:
- `/usr/share/wayland-sessions/lacs-shell.desktop`

Starts Tauri app as the session shell.

---

## 9. Testing & validation

### 9.1 VM test harness

Use QEMU to run Silverblue and execute API calls via a test client.

Golden tests:
1. `InstallFlatpak(org.mozilla.firefox)` then `GetSystemState` verifies presence
2. `RestartService(NetworkManager.service)` returns active
3. `SetDnsServers([1.1.1.1, 8.8.8.8])` and validate NM effective DNS
4. `CollectDiagnostics(wifi_drop)` returns bounded redacted artifacts
5. `UpdateSystem(full)` stages deployment (requires reboot) and verify with `GetSystemState(deployments)`
6. `RollbackDeployment(previous)` stages rollback

Security tests:
- deny unknown service
- deny disallowed remote
- deny invalid IP
- deny disallowed packages
- ensure redaction removes known patterns

### 9.2 E2E UX script

- Install Slack via Flatpak
- Open Slack from app launcher
- Trigger update and show “reboot required”
- Rollback and show “rollback staged; reboot required”

---

## 10. Roadmap (post-MVP)

### 10.1 Expand typed APIs (v2+)

- `ModifyConfig(target,value)` catalog with schema validation
- `ConfigureFirewallRule` (firewalld/nftables)
- `ListOpenPorts` / `AuditSecurityPosture`
- `MountDisk` / `ManageBackups`
- `ManageUsersGroups`
- `RunCommandRestricted(template_id, params)` (escape hatch with templates, approvals, and snapshots)

### 10.2 Policy engine v2

- Policy DSL
- RBAC roles (user/admin/automation)
- Per-network context (untrusted Wi-Fi)
- Time-based rules
- Quotas and anomaly detection

### 10.3 Recovery environment

- Boot-time recovery app:
  - list deployments
  - rollback
  - network debug
  - collect diagnostics offline
  - “factory reset keep /home”

### 10.4 Packaging into a distro

- custom ISO
- default session is `lacs-shell`
- signed updates
- optional Secure Boot story

---

## 11. “Done” criteria (MVP)

- ✅ 12 APIs implemented and documented
- ✅ UI plan preview with approvals
- ✅ Audit timeline with structured results
- ✅ Flatpak installs work without reboot
- ✅ rpm-ostree updates/rollback stage correctly
- ✅ ZeroClaw integration produces Plan objects using only typed APIs
- ✅ No arbitrary root shell exposed anywhere
