# LACS E2E User Stories

Ten scenarios for validating LACS on a real Fedora Atomic Desktop (Silverblue,
Kinoite, Sway Atomic, Budgie Atomic, or COSMIC Atomic). Run inside a QEMU/KVM
VM via `tests/e2e/silverblue-vm.sh`, or on real hardware.

Each story has:

- **Intent** — what the user types into the shell
- **Expected LLM behavior** — which query tools it should call, what plan it
  should propose
- **Automated** — whether the `run-stories.sh` harness exercises it
- **Pass criteria** — concrete conditions for success
- **Cleanup** — how to revert any system changes

The **automated** stories (1–7) are also covered by the container-based CI
smoke test (see `.github/workflows/e2e.yml`). The **semi-automated** stories
(8–10) make real rpm-ostree / filesystem changes and only run when
`LACS_ALLOW_DESTRUCTIVE=1` is set — take a VM snapshot first via
`silverblue-vm.sh snapshot pre-destructive`.

---

## Story 1: Check disk usage

**Persona:** Sysadmin triaging a full disk alert.

**Intent:** `"show me disk usage for all mounted filesystems"`

**Expected LLM behavior:**
- Calls `query_disk_usage` during planning
- Proposes a single-step plan: `GetDiskUsage` (Low risk, no approval required)

**Automated:** yes (read-only)

**Pass criteria:**
- Plan has exactly 1 step, `GetDiskUsage`, risk `low`, `approvalRequired: false`
- Execution returns at least one line matching `/^\/dev\/\S+/` (a real device)
- Execution completes in under 15 seconds

**Cleanup:** none (read-only)

---

## Story 2: Memory pressure diagnosis

**Persona:** Developer whose laptop is sluggish.

**Intent:** `"is the system low on memory? show me what's using it"`

**Expected LLM behavior:**
- Calls `query_memory` and `query_processes` (in either order)
- Proposes a 2-step plan: `GetMemoryInfo` + `ListProcesses`
- Both steps Low risk, no approval required

**Automated:** yes

**Pass criteria:**
- Plan has 2 steps, both risk `low`
- One step is `GetMemoryInfo`, one is `ListProcesses`
- Execution output contains `Mem:` (from `free -h`)
- Execution output contains `PID` (from `ps aux` header)

**Cleanup:** none

---

## Story 3: Service health check

**Persona:** On-call engineer verifying a service.

**Intent:** `"is sshd running? show me its recent logs"`

**Expected LLM behavior:**
- Calls `query_services` to confirm sshd is running
- Calls `query_logs` with param `{unit: "sshd.service"}`
- Proposes a plan with `GetServiceLogs` (param: unit)

**Automated:** yes

**Pass criteria:**
- Plan includes `GetServiceLogs` with `unit` parameter set to `sshd.service` or
  `sshd` (the LLM may or may not add `.service`)
- Execution output contains journal-style log lines
- Execution completes under 20 seconds

**Cleanup:** none

---

## Story 4: Firewall inspection

**Persona:** Security-conscious user before opening a port.

**Intent:** `"what ports are currently open on the firewall?"`

**Expected LLM behavior:**
- Calls `query_firewall` during planning
- Proposes a plan with `GetFirewallState` (Low risk)

**Automated:** yes

**Pass criteria:**
- Plan has 1 step, `GetFirewallState`
- Execution completes without error
- Output contains one of: `services:`, `ports:`, `public (active)`, or similar
  firewalld/iptables markers

**Cleanup:** none

---

## Story 5: List layered packages

**Persona:** Power user recalling what they installed.

**Intent:** `"what packages have I layered on top of the base system?"`

**Expected LLM behavior:**
- Calls `query_packages` during planning
- Proposes `GetLayeredPackages` (Low risk)

**Automated:** yes

**Pass criteria:**
- Plan has 1 step, `GetLayeredPackages`
- Execution completes; empty output is acceptable (no layered packages is a
  valid state)

**Cleanup:** none

---

## Story 6: Running containers overview

**Persona:** Developer checking their podman workflow.

**Intent:** `"list all running containers and show me which services are up"`

**Expected LLM behavior:**
- Calls `query_containers` and `query_services`
- Proposes a 2-step plan: `ListContainers` + `ListServices`

**Automated:** yes

**Pass criteria:**
- Plan has 2 steps, both risk `low`
- `ListContainers` and `ListServices` both present
- Execution output contains `NAMES` (podman ps header) and service names

**Cleanup:** none

---

## Story 7: SSH key inventory

**Persona:** Sysadmin auditing SSH access for a user.

**Intent:** `"show me the SSH keys authorized for user lacsdev"`

**Expected LLM behavior:**
- Calls `query_authorized_keys` with `{username: "lacsdev"}`
- Proposes `GetAuthorizedKeys` with the username parameter

**Automated:** yes

**Pass criteria:**
- Plan has 1 step, `GetAuthorizedKeys`
- `params.username == "lacsdev"` (the test VM has this user pre-provisioned)
- Execution returns the pre-seeded public key (an `ssh-ed25519 AAAA...` line)

**Cleanup:** none

---

## Story 8 (destructive): Layer vim via rpm-ostree

**Persona:** Developer who just realized vim isn't installed.

**Intent:** `"install vim as a layered package"`

**Expected LLM behavior:**
- May call `query_packages` first to check if vim is already layered
- Proposes `InstallPackages` (or `AddLayeredPackage`) with `packages: ["vim"]`
- Plan marked `approvalRequired: true`, risk `high`

**Automated:** only with `LACS_ALLOW_DESTRUCTIVE=1` and a VM snapshot set

**Pass criteria:**
- Plan requires approval (high risk)
- After auto-approval, daemon executes `rpm-ostree install vim`
- Execution succeeds with `needs_reboot` outcome
- `rpm-ostree status` shows vim in staged deployment layered packages

**Cleanup:** revert VM snapshot after the test

---

## Story 9 (destructive): Create a toolbox

**Persona:** Developer setting up a dev environment.

**Intent:** `"create a toolbox container called dev-test for development work"`

**Expected LLM behavior:**
- May call `query_toolboxes` first to check for name collision
- Proposes `CreateToolbox` with `name: "dev-test"`
- Plan marked `approvalRequired: true`, risk `medium`

**Automated:** only with `LACS_ALLOW_DESTRUCTIVE=1`

**Pass criteria:**
- Plan has `CreateToolbox` step with `params.name == "dev-test"`
- After auto-approval, `toolbox list --containers` includes `dev-test`

**Cleanup:**
- Run `toolbox rm -f dev-test` in post-test cleanup
- Or revert VM snapshot

---

## Story 10 (destructive): Add SSH authorized key

**Persona:** Sysadmin provisioning a new admin's access.

**Intent:** `"authorize this SSH key for user lacsdev: ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeTestKeyForE2ETesting testkey@example"`

**Expected LLM behavior:**
- Proposes `AddAuthorizedKey` with `{username: "lacsdev", public_key: "ssh-ed25519 ..."}`
- Plan marked `approvalRequired: true`, risk `medium`
- The public_key param must NOT be truncated or modified

**Automated:** only with `LACS_ALLOW_DESTRUCTIVE=1`

**Pass criteria:**
- Plan has exactly the `AddAuthorizedKey` step
- `params.public_key` matches the user's input verbatim
- After auto-approval, `/home/lacsdev/.ssh/authorized_keys` contains the key
- Idempotency: running the same story twice does not duplicate the entry

**Cleanup:**
- Run `RemoveAuthorizedKey` with the same key, OR
- Revert VM snapshot

---

## Not covered by these stories (document as manual QA)

The following require real hardware or user interaction and should be covered
by the manual QA checklist (see `demo-script.md`):

- **Rollback execution** — deliberately failing a high-risk action and
  verifying automatic rollback (requires flaky hardware or fault injection)
- **RebaseSystem** — full OS upgrade (requires real network, 20+ min)
- **RebootSystem** — actual reboot (breaks VM test flow)
- **Tauri GUI rendering** — the shell's React UI (requires display server;
  covered by `pnpm test` at component level)
- **Reconnect banner on daemon crash** — covered by unit tests

## Running the stories

### Locally (real Silverblue VM, all 10 stories)

```sh
# One-time: download the Fedora Silverblue ISO and install it in QEMU/KVM
./tests/e2e/silverblue-vm.sh download
./tests/e2e/silverblue-vm.sh install

# Every run: boot, provision (rsyncs repo + builds LACS), run stories
./tests/e2e/silverblue-vm.sh start
./tests/e2e/silverblue-vm.sh provision
./tests/e2e/silverblue-vm.sh run

# Destructive stories — snapshot first, then revert
./tests/e2e/silverblue-vm.sh stop && ./tests/e2e/silverblue-vm.sh snapshot clean
./tests/e2e/silverblue-vm.sh start
LACS_ALLOW_DESTRUCTIVE=1 ./tests/e2e/silverblue-vm.sh run
./tests/e2e/silverblue-vm.sh stop && ./tests/e2e/silverblue-vm.sh restore clean
```

See [docs/contributing/testing.md](../contributing/testing.md) for
installation prerequisites, Windows instructions, and troubleshooting.

### In CI

See `.github/workflows/e2e.yml`. Triggered manually via `workflow_dispatch` or
on PRs labeled `e2e`.

### Interpreting results

`run-stories.sh` prints a summary table:

```
Story 1 (Check disk usage):            PASS (3.2s)
Story 2 (Memory pressure diagnosis):   PASS (5.1s)
Story 3 (Service health check):        FAIL (plan missing GetServiceLogs)
...
Summary: 6/7 passed
```

Each story writes detailed logs to `tests/e2e/logs/story-N.log`.
