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
#   download    — fetch the Fedora Atomic ISO (idempotent)
#   install     — start the VM to run the Fedora installer once
#   enable-ssh  — one-time step after install: boot visibly so you can
#                 enable sshd + firewalld (Silverblue ships sshd disabled)
#   keygen      — generate a passphrase-less SSH key dedicated to the VM
#                 (default location: ~/.ssh/lacs-vm)
#   bootstrap   — patch the freshly-installed disk offline (VM must be
#                 stopped): create user, set passwords + sudoers, install
#                 SSH key, enable sshd, set SELinux permissive, skip
#                 gnome-initial-setup. Idempotent.
#   install-key — alias for `bootstrap` (kept for older docs)
#   start       — boot the installed VM headlessly with SSH forwarding
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
#   LACS_VM_MEM      — VM RAM (default: 14G; appended to .conf on download).
#                      Needs to fit the qwen3:14b test model (~9 GB) + OS
#                      overhead (~2 GB) + planning headroom. Reduce to 8G
#                      if you use qwen3:8b, or 16G for qwen3:30b-a3b.
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

# Dedicated passphrase-less SSH key for the VM. We do NOT reuse the
# contributor's personal ~/.ssh/id_* keys because those are typically
# passphrase-protected, which breaks rsync/non-interactive ssh.
SSH_KEY="${LACS_VM_SSH_KEY:-$HOME/.ssh/lacs-vm}"

ssh_opts() {
    printf -- '-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -i %s -o IdentitiesOnly=yes' "$SSH_KEY"
}

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
ram="${LACS_VM_MEM:-14G}"
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
    [ -f "$SSH_KEY" ] || die "SSH key $SSH_KEY not found. Run '$0 keygen' first."
    log "Booting VM headlessly (display=none) in the background..."
    (cd "$VM_DIR" && quickemu --vm "$CONF_NAME" --display none) &
    local port
    port="$(vm_ssh_port)"
    # Wait for real SSH key handshake. qemu's SLIRP hostfwd accepts TCP
    # before sshd is up, so a TCP probe is misleading. We need an actual
    # ssh key auth handshake. Two passes:
    #   1) Try our dedicated key — succeeds when authorized_keys is set up.
    #   2) Fall back to recognising "Permission denied" responses, which
    #      mean sshd is up but our key isn't installed — still counts as
    #      "VM ready", just provisioning hasn't run.
    local max_wait=180 waited=0
    # shellcheck disable=SC2046
    while [ $waited -lt $max_wait ]; do
        if ssh $(ssh_opts) -o BatchMode=yes -o ConnectTimeout=5 \
               -p "$port" "${VM_USER}@127.0.0.1" true 2>/dev/null; then
            log "SSH key auth OK on port $port"
            return 0
        fi
        if ssh $(ssh_opts) -o BatchMode=yes -o ConnectTimeout=5 \
               -p "$port" "${VM_USER}@127.0.0.1" true 2>&1 \
               | grep -qE 'Permission denied|publickey|password'; then
            log "sshd responding on port $port (key not installed; run '$0 install-key')"
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
    # shellcheck disable=SC2046
    exec ssh $(ssh_opts) -p "$port" "${VM_USER}@127.0.0.1" "$@"
}

cmd_provision() {
    require_tools rsync
    [ -f "$SSH_KEY" ] || die "SSH key $SSH_KEY not found. Run '$0 keygen' then '$0 install-key' first."
    local port repo_root
    port="$(vm_ssh_port)"
    repo_root="$(git rev-parse --show-toplevel)"
    log "Copying repo to VM via rsync on port $port..."
    rsync -az --exclude=target --exclude=node_modules --exclude=.git \
        --exclude="$VM_DIR" \
        -e "ssh $(ssh_opts) -p $port" \
        "$repo_root/" "${VM_USER}@127.0.0.1:/home/${VM_USER}/lacs/"
    log "Running provisioner inside the VM..."
    cmd_ssh "cd /home/${VM_USER}/lacs && sudo bash tests/e2e/provision.sh"
}

# Generate a dedicated SSH key for the VM (no passphrase). Idempotent.
cmd_keygen() {
    if [ -f "$SSH_KEY" ]; then
        log "SSH key $SSH_KEY already exists, skipping"
        return
    fi
    log "Generating dedicated VM SSH key at $SSH_KEY (no passphrase)..."
    mkdir -p "$(dirname "$SSH_KEY")"
    ssh-keygen -t ed25519 -N '' -C 'lacs-e2e-vm-only' -f "$SSH_KEY"
    chmod 600 "$SSH_KEY"
}

# Apply all the offline patches the VM needs after Anaconda's install
# but before our headless workflow can touch it. Done via guestfish (no
# need to boot the VM or know any password). Idempotent.
#
# What it does:
#   - Find the rpm-ostree deployment directory (path includes a commit
#     hash that varies per install).
#   - Create user '${VM_USER}' (uid 1000, gid 1000, /bin/bash, wheel group).
#   - Set a password (default '${VM_USER}') so console / serial login works.
#   - Set root password too (default 'lacs') as a fallback.
#   - Install the dedicated SSH key into ~${VM_USER}/.ssh/authorized_keys.
#   - Enable sshd (Silverblue ships it disabled).
#   - NOPASSWD sudoers for ${VM_USER}.
#   - Set SELinux to permissive (we're not testing SELinux).
#   - Pre-mark gnome-initial-setup as done so it doesn't run on first boot.
cmd_bootstrap() {
    require_tools guestfish openssl
    [ -f "$SSH_KEY" ] || die "SSH key $SSH_KEY not found. Run '$0 keygen' first."
    if pgrep -f "qemu-system.*${VM_NAME}" >/dev/null; then
        die "VM is running. Stop it first ('$0 stop' or kill the qemu process)."
    fi
    [ -f "$(vm_disk_path)" ] || die "VM disk not found. Run '$0 install' first."

    local pubkey lacs_hash root_hash
    pubkey="$(cat "${SSH_KEY}.pub")"
    lacs_hash="$(openssl passwd -6 "$VM_USER")"
    root_hash="$(openssl passwd -6 lacs)"

    log "Locating rpm-ostree deployment in disk image..."
    local deploy
    deploy=$(guestfish --ro -a "$(vm_disk_path)" <<EOF | tail -n +1 | tr -d '\r' | head -1
run
mount-options subvol=root /dev/sda3 /
ls /ostree/deploy/fedora/deploy
EOF
)
    # The first line returned should be the commit dir; .origin lines are siblings
    deploy=$(echo "$deploy" | grep -v '\.origin$' | head -1)
    [ -n "$deploy" ] || die "Could not find rpm-ostree deployment under /ostree/deploy/fedora/deploy"
    local deploy_path="/ostree/deploy/fedora/deploy/${deploy}"
    log "Deployment: $deploy_path"

    log "Applying offline patches..."
    guestfish -a "$(vm_disk_path)" <<EOF
run
mount-options subvol=root /dev/sda3 /
mount-options subvol=home /dev/sda3 /home

# 1. /etc/passwd — append ${VM_USER} if not present
copy-out ${deploy_path}/etc/passwd /tmp
! grep -q "^${VM_USER}:" /tmp/passwd || true
! grep -q "^${VM_USER}:" /tmp/passwd || sed -i "/^${VM_USER}:/d" /tmp/passwd
! echo "${VM_USER}:x:1000:1000:LACS Dev:/home/${VM_USER}:/bin/bash" >> /tmp/passwd
upload /tmp/passwd ${deploy_path}/etc/passwd

# 2. /etc/shadow — set root + ${VM_USER} passwords
copy-out ${deploy_path}/etc/shadow /tmp
! sed -i 's|^root:[^:]*:|root:${root_hash}:|' /tmp/shadow
! sed -i "/^${VM_USER}:/d" /tmp/shadow
! echo "${VM_USER}:${lacs_hash}:20000:0:99999:7:::" >> /tmp/shadow
upload /tmp/shadow ${deploy_path}/etc/shadow

# 3. /etc/group — primary group + wheel
copy-out ${deploy_path}/etc/group /tmp
! grep -q "^${VM_USER}:" /tmp/group || sed -i "/^${VM_USER}:/d" /tmp/group
! echo "${VM_USER}:x:1000:" >> /tmp/group
! sed -i "s|^wheel:x:10:.*|wheel:x:10:${VM_USER}|" /tmp/group
upload /tmp/group ${deploy_path}/etc/group

# 4. NOPASSWD sudoers
write ${deploy_path}/etc/sudoers.d/${VM_USER} "${VM_USER} ALL=(ALL) NOPASSWD: ALL\n"
chmod 0440 ${deploy_path}/etc/sudoers.d/${VM_USER}

# 5. Enable sshd via systemd preset symlink
mkdir-p ${deploy_path}/etc/systemd/system/multi-user.target.wants
ln-sf /usr/lib/systemd/system/sshd.service ${deploy_path}/etc/systemd/system/multi-user.target.wants/sshd.service

# 6. SELinux permissive — we don't test SELinux semantics, and our offline
# /etc edits skip the relabel that selinux-enforcing mode requires.
copy-out ${deploy_path}/etc/selinux/config /tmp
! sed -i 's|^SELINUX=enforcing|SELINUX=permissive|' /tmp/selinux-config
! mv /tmp/config /tmp/selinux-config 2>/dev/null || true
upload /tmp/selinux-config ${deploy_path}/etc/selinux/config

# 7. Home + .ssh + authorized_keys
mkdir-p /home/${VM_USER}/.ssh
write /home/${VM_USER}/.ssh/authorized_keys "${pubkey}\n"
chmod 0700 /home/${VM_USER}/.ssh
chmod 0600 /home/${VM_USER}/.ssh/authorized_keys
chown 1000 1000 /home/${VM_USER}
chown 1000 1000 /home/${VM_USER}/.ssh
chown 1000 1000 /home/${VM_USER}/.ssh/authorized_keys

# 8. Skip gnome-initial-setup
mkdir-p /home/${VM_USER}/.config
write /home/${VM_USER}/.config/gnome-initial-setup-done "yes\n"
chown 1000 1000 /home/${VM_USER}/.config
chown 1000 1000 /home/${VM_USER}/.config/gnome-initial-setup-done

# 9. Verify
echo "--- bootstrapped /etc/passwd ${VM_USER} entry ---"
read-file ${deploy_path}/etc/passwd | grep "^${VM_USER}:"
EOF
    log "Bootstrap complete. Boot the VM with '$0 start'."
}

# Backwards-compatible alias (older docs/cmd).
cmd_install_key() {
    log "Note: 'install-key' is now a subset of 'bootstrap'. Running bootstrap..."
    cmd_bootstrap "$@"
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
    keygen)         cmd_keygen "$@" ;;
    install-key)    cmd_install_key "$@" ;;
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
