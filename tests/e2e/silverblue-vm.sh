#!/usr/bin/env bash
#
# silverblue-vm.sh — boot a real Fedora Silverblue VM for LACS E2E testing.
#
# Uses quickemu to download the official Silverblue ISO and run it as a
# QEMU/KVM VM with SSH port forwarding. Works on Linux and macOS hosts.
# Windows contributors: see docs/contributing/testing.md for the manual
# VirtualBox path.
#
# This is the HIGH-FIDELITY path. The VM is a real atomic desktop with
# rpm-ostree, systemd, flatpak, podman, and toolbox — all 10 user stories
# (including destructive ones) execute authentically.
#
# Subcommands:
#   download   — fetch the Silverblue ISO (idempotent)
#   install    — start the VM to run the Fedora installer once
#   start      — boot the installed VM headlessly with SSH forwarding
#   ssh        — open an SSH shell into the VM
#   provision  — copy the repo, run tests/e2e/provision.sh
#   run        — run the story harness (reads LACS_ALLOW_DESTRUCTIVE)
#   snapshot   — create a named qcow2 snapshot before destructive tests
#   restore    — restore the VM to the named snapshot
#   stop       — shut down the VM
#   destroy    — remove the VM disk image (ISO is kept)
#
# Environment:
#   LACS_VM_RELEASE    — Fedora release number (default: 42)
#   LACS_VM_VARIANT    — atomic variant: silverblue | kinoite | sway-atomic |
#                        budgie-atomic | cosmic-atomic (default: silverblue)
#   LACS_VM_DIR        — where to store the ISO + qcow2 (default: tests/e2e/vm)
#   LACS_VM_MEM        — VM RAM, e.g. "4G" (default: 4G)
#   LACS_VM_CPUS       — VM CPU count (default: 2)

set -euo pipefail

RELEASE="${LACS_VM_RELEASE:-42}"
VARIANT="${LACS_VM_VARIANT:-silverblue}"
VM_DIR="${LACS_VM_DIR:-tests/e2e/vm}"
VM_MEM="${LACS_VM_MEM:-4G}"
VM_CPUS="${LACS_VM_CPUS:-2}"

CONF_NAME="fedora-${RELEASE}-${VARIANT}.conf"
CONF_PATH="${VM_DIR}/${CONF_NAME}"

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

vm_ssh_port() {
    # quickemu writes ssh_port to <vm-dir>/<vm-name>.ports after first boot.
    local ports_file="${VM_DIR}/fedora-${RELEASE}-${VARIANT}/fedora-${RELEASE}-${VARIANT}.ports"
    if [ -f "$ports_file" ]; then
        grep -E '^ssh,' "$ports_file" | head -n1 | cut -d, -f2
    else
        echo "22220"
    fi
}

wait_for_ssh() {
    local port="$1"
    local max_wait=60
    local waited=0
    while ! nc -z 127.0.0.1 "$port" 2>/dev/null; do
        if [ "$waited" -ge "$max_wait" ]; then
            die "SSH port $port did not open within ${max_wait}s. Is the VM up? Is sshd installed?"
        fi
        sleep 2
        waited=$((waited + 2))
    done
    log "SSH reachable on port $port"
}

# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

cmd_download() {
    require_tools quickget
    mkdir -p "$VM_DIR"
    cd "$VM_DIR"
    if [ -f "$CONF_NAME" ]; then
        log "Config $CONF_NAME already present, skipping download"
        return
    fi
    log "Downloading Fedora $RELEASE $VARIANT ISO (may be 2-3 GB)..."
    quickget fedora "$RELEASE" "$VARIANT"
    log "Done. Config: $CONF_PATH"
    log "Next: $0 install"
}

cmd_install() {
    require_tools quickemu
    [ -f "$CONF_PATH" ] || die "Config not found at $CONF_PATH. Run: $0 download"
    log "Starting VM to run the Fedora installer (GUI window will open)."
    log "Complete the installation interactively, then shut down the VM."
    log "Suggested: create user 'lacsdev' with password 'lacsdev' to match test expectations."
    (cd "$VM_DIR" && quickemu --vm "$CONF_NAME")
}

cmd_start() {
    require_tools quickemu
    [ -f "$CONF_PATH" ] || die "Config not found at $CONF_PATH. Run: $0 download && $0 install"
    log "Booting VM headlessly (display=none)..."
    # Use --display none for headless boot after initial install is done.
    (cd "$VM_DIR" && quickemu --vm "$CONF_NAME" --display none) &
    local port
    port="$(vm_ssh_port)"
    wait_for_ssh "$port"
    log "VM running. SSH via: ssh -p $port lacsdev@127.0.0.1"
}

cmd_ssh() {
    local port
    port="$(vm_ssh_port)"
    exec ssh \
        -o StrictHostKeyChecking=no \
        -o UserKnownHostsFile=/dev/null \
        -p "$port" \
        "lacsdev@127.0.0.1" "$@"
}

cmd_provision() {
    require_tools rsync
    local port repo_root
    port="$(vm_ssh_port)"
    repo_root="$(git rev-parse --show-toplevel)"
    log "Copying repo to VM via rsync on port $port..."
    rsync -az --exclude=target --exclude=node_modules --exclude=.git \
        -e "ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -p $port" \
        "$repo_root/" "lacsdev@127.0.0.1:/home/lacsdev/lacs/"
    log "Running provisioner inside the VM..."
    cmd_ssh "cd /home/lacsdev/lacs && sudo bash tests/e2e/provision.sh"
}

cmd_run() {
    local flags=""
    if [ "${LACS_ALLOW_DESTRUCTIVE:-}" = "1" ]; then
        flags="LACS_ALLOW_DESTRUCTIVE=1"
        log "Running ALL stories (1-10). Make sure you have a VM snapshot."
    else
        log "Running read-only stories (1-7). Set LACS_ALLOW_DESTRUCTIVE=1 for 8-10."
    fi
    cmd_ssh "cd /home/lacsdev/lacs && sudo -E $flags bash tests/e2e/run-stories.sh"
}

cmd_snapshot() {
    require_tools qemu-img
    local name="${1:-pre-destructive}"
    local disk="${VM_DIR}/fedora-${RELEASE}-${VARIANT}/disk.qcow2"
    [ -f "$disk" ] || die "VM disk not found at $disk. Has the VM been installed?"
    log "Creating internal qcow2 snapshot '$name' (VM must be stopped)..."
    qemu-img snapshot -c "$name" "$disk"
    log "Snapshot created: $name"
}

cmd_restore() {
    require_tools qemu-img
    local name="${1:-pre-destructive}"
    local disk="${VM_DIR}/fedora-${RELEASE}-${VARIANT}/disk.qcow2"
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
    # Wait for the SSH port to close
    local waited=0
    while nc -z 127.0.0.1 "$port" 2>/dev/null; do
        if [ "$waited" -ge 60 ]; then
            log "VM did not shut down cleanly within 60s — may need manual kill"
            break
        fi
        sleep 2
        waited=$((waited + 2))
    done
    log "VM stopped"
}

cmd_destroy() {
    local vm_subdir="${VM_DIR}/fedora-${RELEASE}-${VARIANT}"
    [ -d "$vm_subdir" ] || die "VM directory not found at $vm_subdir"
    log "Removing VM disk and state (keeping the downloaded ISO)..."
    rm -rf "$vm_subdir"
    log "Destroyed. Run '$0 install' to start fresh."
}

cmd_help() {
    sed -n '3,36p' "$0" | sed 's/^# \?//'
}

# ---------------------------------------------------------------------------
# Dispatch
# ---------------------------------------------------------------------------

cmd="${1:-help}"
shift || true

case "$cmd" in
    download)  cmd_download "$@" ;;
    install)   cmd_install "$@" ;;
    start)     cmd_start "$@" ;;
    ssh)       cmd_ssh "$@" ;;
    provision) cmd_provision "$@" ;;
    run)       cmd_run "$@" ;;
    snapshot)  cmd_snapshot "$@" ;;
    restore)   cmd_restore "$@" ;;
    stop)      cmd_stop "$@" ;;
    destroy)   cmd_destroy "$@" ;;
    help|--help|-h) cmd_help ;;
    *) die "unknown command: $cmd. Try: $0 help" ;;
esac
