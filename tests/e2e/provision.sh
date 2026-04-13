#!/usr/bin/env bash
# LACS E2E VM provisioning script.
# Runs inside the Vagrant VM as root.
# Installs all dependencies, builds LACS, and starts the daemon.
set -euo pipefail

MARKER="/var/lib/lacs-e2e/ready"
LOG="/var/log/lacs-e2e-provision.log"

# Remove stale marker from a previous run.
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
# 1. System packages
# ---------------------------------------------------------------------------
step "Install system packages"
dnf install -y \
  git curl jq file \
  gcc gcc-c++ make openssl-devel pkg-config \
  nodejs npm \
  systemd-journal-remote \
  firewalld \
  podman toolbox flatpak \
  || fail "Install system packages"

# rpm-ostree-client may not exist on non-Silverblue; install if available.
# On plain Fedora this is a no-op / soft failure.
dnf install -y rpm-ostree 2>/dev/null || echo "NOTE: rpm-ostree not available (expected on non-Silverblue)"

# ---------------------------------------------------------------------------
# 2. Install Rust via rustup (if not already present)
# ---------------------------------------------------------------------------
step "Install Rust toolchain"
if ! command -v cargo &>/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
# Make cargo available for the rest of this script.
# shellcheck disable=SC1091
source "$HOME/.cargo/env" 2>/dev/null || true
export PATH="$HOME/.cargo/bin:$PATH"
cargo --version || fail "Rust installation"

# ---------------------------------------------------------------------------
# 3. Install Ollama
# ---------------------------------------------------------------------------
step "Install Ollama"
if ! command -v ollama &>/dev/null; then
  curl -fsSL https://ollama.com/install.sh | sh || fail "Ollama installation"
fi

# Start the Ollama service so we can pull a model.
systemctl enable --now ollama 2>/dev/null || {
  # Fallback: start in the background if systemd unit is not available.
  nohup ollama serve &>/var/log/ollama.log &
  sleep 3
}

# ---------------------------------------------------------------------------
# 4. Pull a small LLM for testing
# ---------------------------------------------------------------------------
step "Pull test LLM model"
# qwen3:0.6b is small enough to run on CPU in reasonable time (~600 MB).
# For more reliable planning, use qwen3:1.7b or gemma3:1b — but they are
# slower on CPU-only VMs and require more RAM.
LACS_TEST_MODEL="${LACS_TEST_MODEL:-qwen3:0.6b}"
ollama pull "$LACS_TEST_MODEL" || fail "Model pull ($LACS_TEST_MODEL)"
echo "Model $LACS_TEST_MODEL ready."

# ---------------------------------------------------------------------------
# 5. Build LACS from synced folder
# ---------------------------------------------------------------------------
step "Build LACS"
cd /vagrant

# Build the daemon (release mode, but without --locked since the VM may
# have a slightly different toolchain than the lockfile was generated with).
cargo build --release -p lacs-daemon || fail "Build lacs-daemon"

# Build the E2E test CLI.
cargo build --release -p lacs-test-cli || fail "Build lacs-test-cli"

echo "Binaries:"
ls -lh target/release/lacs-daemon target/release/lacs-test-cli

# ---------------------------------------------------------------------------
# 6. Install the daemon
# ---------------------------------------------------------------------------
step "Install daemon"
make install || fail "make install"

# ---------------------------------------------------------------------------
# 7. Create test user 'lacsdev'
# ---------------------------------------------------------------------------
step "Create test user lacsdev"
if ! id lacsdev &>/dev/null; then
  useradd -m -s /bin/bash lacsdev
fi

# Seed an SSH public key for story 7 (SSH key inventory).
LACSDEV_SSH_DIR="/home/lacsdev/.ssh"
mkdir -p "$LACSDEV_SSH_DIR"
chmod 700 "$LACSDEV_SSH_DIR"

# Generate a deterministic test keypair if not already present.
SEED_KEY="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILacsE2ETestKeyDoNotUseInProduction lacsdev@e2e-test"
if ! grep -qF "$SEED_KEY" "$LACSDEV_SSH_DIR/authorized_keys" 2>/dev/null; then
  echo "$SEED_KEY" >> "$LACSDEV_SSH_DIR/authorized_keys"
fi
chmod 600 "$LACSDEV_SSH_DIR/authorized_keys"
chown -R lacsdev:lacsdev "$LACSDEV_SSH_DIR"
echo "lacsdev SSH key seeded."

# ---------------------------------------------------------------------------
# 8. Configure firewall
# ---------------------------------------------------------------------------
step "Configure firewall"
systemctl enable --now firewalld || fail "Start firewalld"
firewall-cmd --permanent --add-service=ssh 2>/dev/null || true
firewall-cmd --reload || true
echo "Firewall active with ssh service."

# ---------------------------------------------------------------------------
# 9. Start the LACS daemon
# ---------------------------------------------------------------------------
step "Start LACS daemon"
systemctl enable --now lacs-daemon || fail "Start lacs-daemon"
sleep 1
systemctl is-active lacs-daemon || fail "lacs-daemon not running"
echo "lacs-daemon is running."

# ---------------------------------------------------------------------------
# 10. Install lacs-test-cli to PATH
# ---------------------------------------------------------------------------
step "Install lacs-test-cli"
install -m 755 /vagrant/target/release/lacs-test-cli /usr/local/bin/lacs-test-cli
echo "lacs-test-cli installed to /usr/local/bin."

# ---------------------------------------------------------------------------
# 11. Write ready marker
# ---------------------------------------------------------------------------
step "Write ready marker"
mkdir -p /var/lib/lacs-e2e
date --iso-8601=seconds > "$MARKER"
echo ""
echo "================================================================"
echo "  PROVISIONING COMPLETE"
echo "  Ready marker: $MARKER"
echo "  Ollama model: $LACS_TEST_MODEL"
echo "  Run stories:  cd /vagrant && sudo tests/e2e/run-stories.sh"
echo "================================================================"
