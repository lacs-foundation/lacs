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

# Start Ollama. The installer typically creates a systemd unit.
systemctl enable --now ollama 2>/dev/null || {
    nohup ollama serve &>/var/log/ollama.log &
    sleep 3
}

# ---------------------------------------------------------------------------
# Phase 2: Pull a small LLM
# ---------------------------------------------------------------------------

step "Pull test LLM model"
LACS_TEST_MODEL="${LACS_TEST_MODEL:-qwen3:0.6b}"
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
make install || fail "make install"

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
