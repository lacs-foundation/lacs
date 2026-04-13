#!/usr/bin/env bash
#
# silverblue-vm.sh — boot a real Fedora Atomic Desktop VM for LACS E2E testing.
#
# Uses quickemu to download the official Fedora ISO and run it as a
# QEMU/KVM VM with SSH port forwarding. Works on Linux and macOS hosts.
# Windows contributors: see docs/contributing/testing.md for the manual
# VirtualBox path.
#
# This is the HIGH-FIDELITY path. The VM is a real atomic desktop with
# rpm-ostree, systemd, flatpak, podman, and toolbox — all 10 user stories
# (including destructive ones) execute authentically.
#
# Subcommands:
#   download   — fetch the Fedora Atomic ISO (idempotent)
#   install    — start the VM to run the Fedora installer once
#   enable-ssh — one-time step after install: boot visibly so you can
#                enable sshd + firewalld (Silverblue ships sshd disabled)
#   start      — boot the installed VM headlessly with SSH forwarding
#   ssh        — open an SSH shell into the VM (or run a command)
#   provision  — rsync the repo, run tests/e2e/provision.sh inside the VM
#   run        — run the story harness (reads LACS_ALLOW_DESTRUCTIVE)
#   snapshot   — create a named qcow2 snapshot before destructive tests
#   restore    — restore the VM to the named snapshot
#   stop       — shut down the VM
#   destroy    — remove the VM disk image (ISO is kept)
#   help       — print this help
#
# Environment:
#   LACS_VM_RELEASE  — Fedora release number (default: 42)
#   LACS_VM_VARIANT  — atomic variant. Accepted values (case-insensitive):
#                      silverblue (GNOME), kinoite (KDE),
#                      sericea (Sway), onyx (Budgie).
#                      Default: silverblue.
#                      Note: COSMIC Atomic is not yet in quickget.
#   LACS_VM_DIR      — where to store the ISO + qcow2 (default: tests/e2e/vm)
#   LACS_VM_USER     — VM user created by the installer (default: lacsdev)
#   LACS_VM_MEM      — VM RAM (default: 6G; appended to .conf on download)
#   LACS_VM_CPUS     — VM CPU count (default: 4; appended to .conf on download)
#   LACS_VM_DISK     — VM disk size (default: 40G; appended to .conf on download)

set -euo pipefail

RELEASE="${LACS_VM_RELEASE:-42}"
# Normalize to lowercase for path consistency; quickget accepts any case.
VARIANT="$(printf '%s' "${LACS_VM_VARIANT:-silverblue}" | tr '[:upper:]' '[:lower:]')"
VM_DIR="${LACS_VM_DIR:-tests/e2e/vm}"
VM_USER="${LACS_VM_USER:-lacsdev}"

# quickget's canonical capitalized edition name for the `quickget` CLI.
# quickget writes the config file with the edition lowercased.
case "$VARIANT" in
    silverblue) QUICKGET_EDITION="Silverblue" ;;
    kinoite)    QUICKGET_EDITION="Kinoite" ;;
    sericea)    QUICKGET_EDITION="Sericea" ;;   # Sway Atomic
    onyx)       QUICKGET_EDITION="Onyx" ;;      # Budgie Atomic
    *)
        echo "[silverblue-vm] ERROR: unknown LACS_VM_VARIANT='$VARIANT'." >&2
        echo "  Accepted: silverblue | kinoite | sericea | onyx" >&2
        exit 1
        ;;
esac

# quickget builds VM_PATH as `${OS}-${RELEASE}-${EDITION}` with the
# edition capitalization preserved (verified against quickget source line 4024).
# So config and VM dir end up at:
#   <cwd>/fedora-<release>-<Edition>.conf
#   <cwd>/fedora-<release>-<Edition>/
# where <Edition> is the canonical Capitalized name (Silverblue, Kinoite, ...).
VM_NAME="fedora-${RELEASE}-${QUICKGET_EDITION}"
CONF_NAME="${VM_NAME}.conf"
CONF_PATH="${VM_DIR}/${CONF_NAME}"
VM_SUBDIR="${VM_DIR}/${VM_NAME}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

log() { printf '[silverblue-vm] %s\n' "$*" >&2; }
die() { log "ERROR: $*"; exit 1; }

require_tools() {
    local missing=()
    for tool in "$@"; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            missing+=("$tool")
        fi
    done
    if [ ${#missing[@]} -gt 0 ]; then
        die "missing required tools: ${missing[*]}. See docs/contributing/testing.md for install instructions."
    fi
}

# Return the host TCP port forwarded to the guest's SSH (auto-assigned by
# quickemu from the 22220-22229 range). The ports file is at
# <vm-subdir>/<vm-name>.ports with one entry per line like "ssh,22220".
vm_ssh_port() {
    local ports_file="${VM_SUBDIR}/${VM_NAME}.ports"
    if [ -f "$ports_file" ]; then
        local port
        port="$(awk -F, '/^ssh,/ {print $2; exit}' "$ports_file" | tr -d '[:space:]')"
        if [ -n "$port" ]; then
            echo "$port"
            return
        fi
    fi
    # Fall back to the first port of quickemu's default range.
    echo "22220"
}

wait_for_ssh() {
    local port="$1"
    local max_wait=120
    local waited=0
    while ! nc -z 127.0.0.1 "$port" 2>/dev/null; do
        if [ "$waited" -ge "$max_wait" ]; then
            die "SSH port $port did not open within ${max_wait}s. Is the VM up? Is sshd enabled in the guest?"
        fi
        sleep 3
        waited=$((waited + 3))
    done
    log "SSH reachable on port $port"
}

# Resolve the VM's qcow2 disk path. quickemu names it "disk.qcow2" inside
# the VM subdirectory.
vm_disk_path() {
    echo "${VM_SUBDIR}/disk.qcow2"
}

# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

cmd_download() {
    require_tools quickget
    mkdir -p "$VM_DIR"
    if [ -f "$CONF_PATH" ]; then
        log "Config $CONF_PATH already present, skipping download"
        return
    fi
    log "Downloading Fedora $RELEASE $QUICKGET_EDITION ISO (may be 2-3 GB)..."
    # quickget writes relative to CWD — run it inside VM_DIR.
    (cd "$VM_DIR" && quickget fedora "$RELEASE" "$QUICKGET_EDITION")
    # quickget produces a minimal config; append our resource overrides so
    # the VM has enough RAM/CPU/disk to build LACS and run a small Ollama model.
    if ! grep -q '^# LACS E2E overrides' "$CONF_PATH"; then
        cat >> "$CONF_PATH" <<EOF

# LACS E2E overrides — appended by silverblue-vm.sh download
disk_size="${LACS_VM_DISK:-40G}"
ram="${LACS_VM_MEM:-6G}"
cpu_cores="${LACS_VM_CPUS:-4}"
# gl="off" — disable virtio-vga-gl/virgl. Fedora 42's gnome-initial-setup
# crashes the QEMU window with a flicker-then-freeze on hosts with hybrid
# graphics (Intel iGPU + NVIDIA dGPU is the common case). Software
# rendering inside the guest is plenty fast for our use.
gl="off"
EOF
    fi
    log "Done. Config: $CONF_PATH"
    log "Next: $0 install"
}

cmd_install() {
    require_tools quickemu
    [ -f "$CONF_PATH" ] || die "Config not found at $CONF_PATH. Run: $0 download"
    cat >&2 <<NOTE
[silverblue-vm] Starting VM with the Fedora installer (GUI window will open).

  During the Anaconda installer:
    1. Pick language → Continue
    2. Root password: set anything (or leave disabled)
    3. User Creation: username '${VM_USER}', password '${VM_USER}',
       ✅ 'Make this user administrator'
    4. Begin Installation → wait ~5-10 min

  After 'Complete!' screen:
    - Close the QEMU window (or run \`sudo poweroff\` in the VM)
    - Do NOT click 'Reboot' — the ISO will re-mount as CD-ROM

  After the installer window closes, run '$0 enable-ssh' to boot the VM
  visibly one more time and turn on sshd + the firewall rule. Silverblue
  ships sshd DISABLED by default; we need it on for provisioning.
NOTE
    (cd "$VM_DIR" && quickemu --vm "$CONF_NAME")
}

# Boots the VM visibly (GTK display) so the user can enable sshd.
# Silverblue ships sshd installed but disabled; we need it on for our
# headless provisioning flow.
cmd_enable_ssh() {
    require_tools quickemu
    [ -f "$CONF_PATH" ] || die "Config not found at $CONF_PATH. Run: $0 install first."
    cat >&2 <<NOTE
[silverblue-vm] Booting VM visibly so you can enable sshd.

  Log in as '${VM_USER}', open a terminal, and run:

    sudo systemctl enable --now sshd
    sudo firewall-cmd --permanent --add-service=ssh
    sudo firewall-cmd --reload
    sudo poweroff

  Then run '$0 start' to boot headless and '$0 provision' to continue.
NOTE
    (cd "$VM_DIR" && quickemu --vm "$CONF_NAME")
}

cmd_start() {
    require_tools quickemu
    [ -f "$CONF_PATH" ] || die "Config not found at $CONF_PATH. Run: $0 download && $0 install"
    log "Booting VM headlessly (display=none) in the background..."
    (cd "$VM_DIR" && quickemu --vm "$CONF_NAME" --display none) &
    local port
    port="$(vm_ssh_port)"
    # Wait for real SSH handshake, not just TCP (qemu SLIRP accepts TCP
    # before sshd is up). A Connection-reset RST at kex_exchange means
    # the guest has no process listening on port 22 — usually because
    # sshd was never enabled. Run '$0 enable-ssh' first if so.
    local max_wait=180 waited=0
    while [ $waited -lt $max_wait ]; do
        if ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
               -o BatchMode=yes -o ConnectTimeout=5 -o LogLevel=ERROR \
               -p "$port" "${VM_USER}@127.0.0.1" true 2>/dev/null; then
            log "SSH handshake OK on port $port"
            return 0
        fi
        # BatchMode=yes will fail with "Permission denied (publickey)" when
        # sshd is up but our key isn't authorized — still counts as "ready".
        if ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
               -o BatchMode=yes -o ConnectTimeout=5 -o LogLevel=ERROR \
               -p "$port" "${VM_USER}@127.0.0.1" true 2>&1 \
               | grep -qE 'Permission denied|publickey|password'; then
            log "sshd responding on port $port (key auth may need 'ssh-copy-id')"
            return 0
        fi
        sleep 5
        waited=$((waited + 5))
    done
    die "sshd did not respond on port $port within ${max_wait}s. If the VM is booted but SSH is refusing, run '$0 enable-ssh' to turn sshd on inside the guest."
}

cmd_ssh() {
    local port
    port="$(vm_ssh_port)"
    exec ssh \
        -o StrictHostKeyChecking=no \
        -o UserKnownHostsFile=/dev/null \
        -o LogLevel=ERROR \
        -p "$port" \
        "${VM_USER}@127.0.0.1" "$@"
}

cmd_provision() {
    require_tools rsync
    local port repo_root
    port="$(vm_ssh_port)"
    repo_root="$(git rev-parse --show-toplevel)"
    log "Copying repo to VM via rsync on port $port..."
    rsync -az --exclude=target --exclude=node_modules --exclude=.git \
        --exclude="$VM_DIR" \
        -e "ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -p $port" \
        "$repo_root/" "${VM_USER}@127.0.0.1:/home/${VM_USER}/lacs/"
    log "Running provisioner inside the VM..."
    cmd_ssh "cd /home/${VM_USER}/lacs && sudo bash tests/e2e/provision.sh"
}

cmd_run() {
    local env_prefix=""
    if [ "${LACS_ALLOW_DESTRUCTIVE:-}" = "1" ]; then
        env_prefix="LACS_ALLOW_DESTRUCTIVE=1"
        log "Running ALL stories (1-10). Make sure you have a VM snapshot."
    else
        log "Running read-only stories (1-7). Set LACS_ALLOW_DESTRUCTIVE=1 for 8-10."
    fi
    cmd_ssh "cd /home/${VM_USER}/lacs && sudo -E $env_prefix bash tests/e2e/run-stories.sh"
}

cmd_snapshot() {
    require_tools qemu-img
    local name="${1:-pre-destructive}"
    local disk
    disk="$(vm_disk_path)"
    [ -f "$disk" ] || die "VM disk not found at $disk. Has the VM been installed?"
    log "Creating internal qcow2 snapshot '$name' (VM must be stopped)..."
    qemu-img snapshot -c "$name" "$disk"
    log "Snapshot created: $name"
}

cmd_restore() {
    require_tools qemu-img
    local name="${1:-pre-destructive}"
    local disk
    disk="$(vm_disk_path)"
    [ -f "$disk" ] || die "VM disk not found at $disk."
    log "Restoring snapshot '$name' (VM must be stopped)..."
    qemu-img snapshot -a "$name" "$disk"
    log "Restored. Start the VM: $0 start"
}

cmd_stop() {
    local port
    port="$(vm_ssh_port)"
    log "Requesting clean shutdown via SSH..."
    cmd_ssh "sudo systemctl poweroff" || true
    # Wait for the SSH port to close.
    local waited=0
    while nc -z 127.0.0.1 "$port" 2>/dev/null; do
        if [ "$waited" -ge 60 ]; then
            log "VM did not shut down cleanly within 60s. You may need to kill the qemu process manually."
            break
        fi
        sleep 2
        waited=$((waited + 2))
    done
    log "VM stopped"
}

cmd_destroy() {
    [ -d "$VM_SUBDIR" ] || die "VM directory not found at $VM_SUBDIR"
    log "Removing VM disk and state (the downloaded ISO is kept)..."
    rm -rf "$VM_SUBDIR"
    log "Destroyed. Run '$0 install' to start fresh."
}

cmd_help() {
    # Print the header comment block (lines 3 through the first blank line
    # before `set -euo pipefail`). Strip the leading "# " comment marker.
    sed -n '3,/^set -euo pipefail$/p' "$0" \
        | sed -e 's/^# \?//' -e '/^set -euo pipefail$/d' -e '/^$/d'
}

# ---------------------------------------------------------------------------
# Dispatch
# ---------------------------------------------------------------------------

cmd="${1:-help}"
shift || true

case "$cmd" in
    download)       cmd_download "$@" ;;
    install)        cmd_install "$@" ;;
    enable-ssh)     cmd_enable_ssh "$@" ;;
    start)          cmd_start "$@" ;;
    ssh)            cmd_ssh "$@" ;;
    provision)      cmd_provision "$@" ;;
    run)            cmd_run "$@" ;;
    snapshot)       cmd_snapshot "$@" ;;
    restore)        cmd_restore "$@" ;;
    stop)           cmd_stop "$@" ;;
    destroy)        cmd_destroy "$@" ;;
    help|--help|-h) cmd_help ;;
    *)              die "unknown command: $cmd. Try: $0 help" ;;
esac
