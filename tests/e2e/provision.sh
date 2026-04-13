#!/usr/bin/env bash
# LACS E2E VM provisioning script.
#
# Runs inside a Fedora Atomic Desktop VM (Silverblue, Kinoite, Sway Atomic,
# Budgie Atomic, or COSMIC Atomic) as root. It layers required build tools
# via rpm-ostree, installs Ollama, pulls a small model, builds LACS from
# the repo copy at $REPO_DIR, and starts the daemon.
#
# The repo copy is expected at $REPO_DIR (default: /home/lacsdev/lacs),
# matching what tests/e2e/silverblue-vm.sh rsyncs over via `provision`.
#
# If rpm-ostree needs to layer packages, it requires a reboot to take
# effect. This script handles the two-phase flow:
#   - First run: layers build tools + reboots
#   - Second run: builds LACS + starts daemon
# A sentinel file at /var/lib/lacs-e2e/layered marks phase 1 complete.

set -euo pipefail

REPO_DIR="${REPO_DIR:-/home/lacsdev/lacs}"
MARKER="/var/lib/lacs-e2e/ready"
LAYERED_MARKER="/var/lib/lacs-e2e/layered"
LOG="/var/log/lacs-e2e-provision.log"

mkdir -p /var/lib/lacs-e2e
rm -f "$MARKER"

# Redirect all output to both the console and the log file.
exec > >(tee -a "$LOG") 2>&1

step() {
    echo ""
    echo "================================================================"
    echo "  STEP: $1"
    echo "================================================================"
}

fail() {
    echo ""
    echo "!!! PROVISIONING FAILED at step: $1"
    echo "!!! Check $LOG for details."
    exit 1
}

# ---------------------------------------------------------------------------
# Phase 1: Layer build tools via rpm-ostree (requires reboot afterward)
# ---------------------------------------------------------------------------

if [ ! -f "$LAYERED_MARKER" ]; then
    step "Layer build tools via rpm-ostree"
    # jq, rsync, nc, podman, toolbox, flatpak are present on atomic desktops.
    # rustup handles rust itself; we only need build prereqs (gcc, etc.).
    # zstd is needed by the Ollama installer script (it ships its tarball
    # zstd-compressed and the install.sh extracts via `unzstd`).
    rpm-ostree install --idempotent --allow-inactive \
        gcc gcc-c++ make openssl-devel pkg-config zstd \
        || fail "Layer build tools"
    touch "$LAYERED_MARKER"
    echo ""
    echo "================================================================"
    echo "  PHASE 1 COMPLETE — rebooting to activate layered packages"
    echo "  After reboot, re-run: sudo bash $0"
    echo "================================================================"
    sleep 3
    systemctl reboot
    exit 0
fi

echo "Phase 1 already complete (found $LAYERED_MARKER). Continuing phase 2."

# ---------------------------------------------------------------------------
# Phase 2: Rust toolchain via rustup (user-local, no rpm-ostree reboot)
# ---------------------------------------------------------------------------

step "Install Rust via rustup"
if ! command -v cargo &>/dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain stable \
        || fail "Install Rust"
fi
# shellcheck disable=SC1091
source "$HOME/.cargo/env" 2>/dev/null || true
export PATH="$HOME/.cargo/bin:$PATH"
cargo --version || fail "Rust install verification"

# ---------------------------------------------------------------------------
# Phase 2: Install Ollama
# ---------------------------------------------------------------------------

step "Install Ollama"
if ! command -v ollama &>/dev/null; then
    curl -fsSL https://ollama.com/install.sh | sh || fail "Ollama install"
fi

# The official installer tries to create a systemd unit + ollama system
# user. On rpm-ostree systems (Silverblue, Kinoite, Sericea, Onyx) that
# can fail because /usr is read-only, or because the install was
# interrupted. Write a minimal unit ourselves if it's missing. Idempotent.
if [ ! -f /etc/systemd/system/ollama.service ] \
    && [ ! -f /usr/lib/systemd/system/ollama.service ]; then
    echo "Ollama systemd unit not found — writing one to /etc/systemd/system/"
    install -d -m 755 -o "$VM_USER" -g "$VM_USER" /var/lib/ollama 2>/dev/null \
        || install -d -m 755 -o lacsdev -g lacsdev /var/lib/ollama
    cat > /etc/systemd/system/ollama.service <<UNIT
[Unit]
Description=Ollama Service
After=network-online.target

[Service]
ExecStart=/usr/local/bin/ollama serve
Environment=HOME=/var/lib/ollama
Environment=OLLAMA_HOST=127.0.0.1:11434
Restart=always
User=${VM_USER:-lacsdev}
Group=${VM_USER:-lacsdev}

[Install]
WantedBy=default.target
UNIT
    systemctl daemon-reload
fi

systemctl enable --now ollama || fail "Start Ollama systemd unit"
# Wait up to 15s for the server to accept connections.
for i in $(seq 1 15); do
    if curl -sf http://127.0.0.1:11434/api/tags > /dev/null; then break; fi
    sleep 1
done
curl -sf http://127.0.0.1:11434/api/tags > /dev/null || fail "Ollama not responding on 11434"

# ---------------------------------------------------------------------------
# Phase 2: Pull a small LLM
# ---------------------------------------------------------------------------

step "Pull test LLM model"
# qwen3:8b is the sweet spot for CPU-only tool calling inside the VM:
# ~5 GB disk, reliable tool calling, ~20-45 s/story on 4 vCPUs.
# We learned this the hard way — qwen3:14b loaded fine but Qwen3's
# default thinking mode pushes CPU-only latency to minutes per story,
# and qwen3:0.6b was too small to emit correct tool calls at all.
#
# Override with LACS_TEST_MODEL:
#   LACS_TEST_MODEL=qwen3:14b    # needs GPU passthrough
#   LACS_TEST_MODEL=qwen3:30b-a3b # MoE, needs 16 GB+ VM RAM
LACS_TEST_MODEL="${LACS_TEST_MODEL:-qwen3:8b}"
ollama pull "$LACS_TEST_MODEL" || fail "Pull $LACS_TEST_MODEL"

# ---------------------------------------------------------------------------
# Phase 2: Build LACS
# ---------------------------------------------------------------------------

step "Build LACS from $REPO_DIR"
[ -d "$REPO_DIR" ] || fail "Repo directory $REPO_DIR not found. Did you run 'silverblue-vm.sh provision'?"
cd "$REPO_DIR"

cargo build --release -p lacs-daemon       || fail "Build lacs-daemon"
cargo build --release -p lacs-test-cli     || fail "Build lacs-test-cli"

echo "Binaries:"
ls -lh target/release/lacs-daemon target/release/lacs-test-cli

# ---------------------------------------------------------------------------
# Phase 2: Install the daemon via Makefile
# ---------------------------------------------------------------------------

step "Install daemon"
# On rpm-ostree systems (Silverblue, Kinoite, Sericea, Onyx) /usr is
# read-only, so the default Makefile paths fail. Detect ostree and redirect
# the systemd / polkit / sysusers / tmpfiles fragments into /etc instead.
if command -v rpm-ostree &>/dev/null && rpm-ostree status --booted &>/dev/null; then
    echo "Detected rpm-ostree host — installing with /etc overrides."
    make install \
        SYSUSERS=/etc/sysusers.d \
        TMPFILES=/etc/tmpfiles.d \
        SYSTEMD=/etc/systemd/system \
        POLKIT=/etc/polkit-1/rules.d \
        || fail "make install (rpm-ostree paths)"
else
    make install || fail "make install"
fi

# ---------------------------------------------------------------------------
# Phase 2: Create test user 'lacsdev' (if not already present from installer)
# ---------------------------------------------------------------------------

step "Set up test user 'lacsdev'"
if ! id lacsdev &>/dev/null; then
    useradd -m -s /bin/bash lacsdev
fi

LACSDEV_SSH_DIR="/home/lacsdev/.ssh"
mkdir -p "$LACSDEV_SSH_DIR"
chmod 700 "$LACSDEV_SSH_DIR"

SEED_KEY="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILacsE2ETestKeyDoNotUseInProduction lacsdev@e2e-test"
if ! grep -qF "$SEED_KEY" "$LACSDEV_SSH_DIR/authorized_keys" 2>/dev/null; then
    echo "$SEED_KEY" >> "$LACSDEV_SSH_DIR/authorized_keys"
fi
chmod 600 "$LACSDEV_SSH_DIR/authorized_keys"
chown -R lacsdev:lacsdev "$LACSDEV_SSH_DIR"

# ---------------------------------------------------------------------------
# Phase 2: Firewall
# ---------------------------------------------------------------------------

step "Configure firewall"
systemctl enable --now firewalld || fail "Start firewalld"
firewall-cmd --permanent --add-service=ssh 2>/dev/null || true
firewall-cmd --reload || true

# ---------------------------------------------------------------------------
# Phase 2: Start the LACS daemon
# ---------------------------------------------------------------------------

step "Start LACS daemon"
systemctl enable --now lacs-daemon || fail "Start lacs-daemon"
sleep 1
systemctl is-active lacs-daemon || fail "lacs-daemon not running"

# ---------------------------------------------------------------------------
# Phase 2: Install lacs-test-cli to PATH
# ---------------------------------------------------------------------------

step "Install lacs-test-cli"
install -m 755 "$REPO_DIR/target/release/lacs-test-cli" /usr/local/bin/lacs-test-cli

# ---------------------------------------------------------------------------
# Phase 2: Write ready marker
# ---------------------------------------------------------------------------

step "Write ready marker"
date --iso-8601=seconds > "$MARKER"
echo ""
echo "================================================================"
echo "  PROVISIONING COMPLETE"
echo "  Ready marker: $MARKER"
echo "  Ollama model: $LACS_TEST_MODEL"
echo "  Run stories:  cd $REPO_DIR && sudo -E tests/e2e/run-stories.sh"
echo "================================================================"
