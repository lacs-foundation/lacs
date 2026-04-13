# Testing LACS

This guide explains how to run LACS tests at every level — from the fast
unit tests you run locally on every change to the VM-based end-to-end
validation before a release.

## Test pyramid

| Level | What it tests | Speed | When |
|---|---|---|---|
| Unit tests (Rust) | Individual functions, parsers, traits | <5s | Every commit, every CI run |
| Unit tests (TypeScript) | React components, reducers, IPC shims | <5s | Every commit, every CI run |
| Integration (Rust) | Daemon IPC, safety fence, policy | <10s | Every commit, every CI run |
| CI smoke (container) | Daemon + Ollama + read-only stories in a Linux runner | 5-10 min | Opt-in (PR label `e2e` or manual trigger) |
| E2E Silverblue VM | **Real Silverblue** in QEMU/KVM, full stack, all 10 stories | 15-30 min first boot; 2-3 min subsequent | Local / pre-release |
| Manual QA | Real Silverblue/Kinoite hardware, destructive actions, GUI | 30-60 min | Before releases + demo video |

No single layer is enough on its own. Use the ones that match your change.

## Running unit and integration tests (required)

These run on every CI build and must pass before merge:

```sh
# Rust
cargo fmt --all --check
cargo clippy --workspace --all-features --locked -- -D warnings
cargo test --workspace --locked

# TypeScript / React
cd apps/lacs-shell
pnpm install --frozen-lockfile
pnpm test
pnpm exec tsc --noEmit
```

## Running the CI smoke test (opt-in)

The smoke test boots Ollama and the daemon directly in a GitHub Actions
runner (no VM, no real atomic desktop), pulls a small tool-capable model
(`gemma3:1b`), and runs the 7 read-only user stories.

**Triggers:**

1. Label a PR with `e2e` — the workflow runs automatically
2. Manual dispatch via Actions → **e2e** → Run workflow

Results appear as the `container-smoke` job. Story logs are uploaded as
build artifacts.

**What the smoke test covers:**

- Daemon startup, IPC framing, policy enforcement
- Brain ↔ Ollama integration, tool-use loop, safety fence
- All 7 read-only query tools and read-only action stories (1–7)

**What it does NOT cover** (that's the VM path below):

- rpm-ostree actions, real systemd host management
- Reboot / kernel-argument flows, rollback execution
- Tauri GUI rendering

## Running the full E2E suite in a Silverblue VM

This is the **high-fidelity** path. The VM is a real Fedora Atomic Desktop
(Silverblue / Kinoite / Sericea / Onyx) install with rpm-ostree, systemd,
flatpak, podman, and toolbox. All 10 user stories — including destructive
ones — execute authentically.

### Linux and macOS hosts (recommended)

We use [quickemu] to download the official Fedora ISO and boot it in
QEMU/KVM with SSH forwarding pre-configured. One-time setup, then a
reproducible VM you can snapshot and restore.

[quickemu]: https://github.com/quickemu-project/quickemu

**Install quickemu:**

You also need `qemu-system-x86_64`, `qemu-utils` (for `qemu-img`), `rsync`,
`netcat`, and `ssh` — these are all standard packages on every supported
distro.

```sh
# Fedora 41+ (default repos have a current quickemu)
sudo dnf install quickemu qemu qemu-img

# Fedora Atomic Desktops (Silverblue / Kinoite / Sericea / Onyx host)
sudo rpm-ostree install quickemu qemu qemu-img
# Reboot to activate, then proceed.

# Ubuntu 24.04 / Debian — the version in default Ubuntu repos may be too
# old (missing the Nov 2024 .ociarchive fix for Fedora Atomic). Use the PPA:
sudo add-apt-repository -y ppa:flexiondotorg/quickemu
sudo apt-get update
sudo apt-get install -y quickemu qemu-system-x86 qemu-utils \
    qemu-system-modules-spice rsync netcat-openbsd

# macOS (Homebrew)
brew install --cask quickemu
```

After installing, verify your user can access KVM (Linux only):

```sh
ls -l /dev/kvm           # should exist
groups | grep -q kvm \
    || sudo usermod -aG kvm "$USER"   # then log out and back in
```

**One-time VM setup:**

```sh
# From the repo root

# 1. Download the Silverblue 42 ISO (~2.5 GB, cached under tests/e2e/vm/)
./tests/e2e/silverblue-vm.sh download

# 2. Run the Fedora installer interactively (GUI window opens).
#    When prompted, create user 'lacsdev' with password 'lacsdev'.
#    Enable sshd from the Services screen during install.
#    Shut the VM down when installation finishes.
./tests/e2e/silverblue-vm.sh install
```

**Run the tests:**

```sh
# Boot the VM headlessly (in the background)
./tests/e2e/silverblue-vm.sh start

# First-ever provision: rsyncs the repo into the VM, layers build tools
# via rpm-ostree, reboots the VM, then builds LACS and starts the daemon.
# Expect ~15 minutes the first time; ~2 minutes on subsequent provisions.
./tests/e2e/silverblue-vm.sh provision

# Take a snapshot BEFORE running destructive stories
./tests/e2e/silverblue-vm.sh stop
./tests/e2e/silverblue-vm.sh snapshot pre-destructive
./tests/e2e/silverblue-vm.sh start

# Run the read-only stories (1-7)
./tests/e2e/silverblue-vm.sh run

# Run ALL stories including destructive (8-10)
LACS_ALLOW_DESTRUCTIVE=1 ./tests/e2e/silverblue-vm.sh run

# Roll back destructive changes via the snapshot
./tests/e2e/silverblue-vm.sh stop
./tests/e2e/silverblue-vm.sh restore pre-destructive
```

**Other useful commands:**

```sh
./tests/e2e/silverblue-vm.sh ssh            # interactive shell in the VM
./tests/e2e/silverblue-vm.sh stop           # clean shutdown
./tests/e2e/silverblue-vm.sh destroy        # delete VM disk (ISO kept)
./tests/e2e/silverblue-vm.sh help
```

**Try a different atomic variant:**

```sh
LACS_VM_VARIANT=kinoite ./tests/e2e/silverblue-vm.sh download
LACS_VM_VARIANT=kinoite ./tests/e2e/silverblue-vm.sh install
# ... all management commands respect LACS_VM_VARIANT.
```

Supported variants (these are the names quickget uses):

| `LACS_VM_VARIANT` | Atomic Desktop | Desktop |
|---|---|---|
| `silverblue` (default) | Fedora Silverblue | GNOME |
| `kinoite` | Fedora Kinoite | KDE Plasma |
| `sericea` | Fedora Sway Atomic | Sway |
| `onyx` | Fedora Budgie Atomic | Budgie |

COSMIC Atomic is not yet packaged by quickget; install it manually from
the ISO if needed.

### Windows hosts

quickemu does not support Windows as a host. Contributors on Windows
should use WSL2 (with KVM nested virtualization) or VirtualBox with a
manual ISO install:

1. Download the Silverblue ISO from
   [fedoraproject.org/atomic-desktops/silverblue](https://fedoraproject.org/atomic-desktops/silverblue/)
2. Create a VirtualBox VM (4 GB RAM, 2 vCPUs, 20 GB disk) with SSH port
   forwarded from host 22220 → guest 22
3. Attach the ISO, boot, and run the Fedora installer. Create user
   `lacsdev`. Enable sshd during install.
4. SSH into the VM: `ssh -p 22220 lacsdev@127.0.0.1`
5. Clone the repo into `/home/lacsdev/lacs` and run
   `sudo bash tests/e2e/provision.sh` inside the VM
6. Run stories with `sudo -E tests/e2e/run-stories.sh`

The `silverblue-vm.sh` helper does not automate VirtualBox — that's a
follow-up if Windows contributor interest warrants it.

## Running individual stories

Inside the VM (or on any provisioned Fedora Atomic Desktop):

```sh
cd /home/lacsdev/lacs

# Run a specific story by number
sudo -E tests/e2e/run-stories.sh 3

# Run multiple specific stories
sudo -E tests/e2e/run-stories.sh 1 4 7
```

Per-story logs are written to `tests/e2e/logs/story-N.log`.

## Before opening a PR

1. `cargo test --workspace && pnpm test` — required, fast
2. `cargo clippy --workspace --all-features --locked -- -D warnings`
3. `cargo fmt --all --check`
4. For changes to the brain, daemon, IPC, or action catalogue:
   - Run the VM tests locally (`silverblue-vm.sh` flow)
   - Add the `e2e` label to trigger the CI smoke test on your PR

## Before a release

The maintainer runs these in order:

1. All automated tests green on main
2. VM tests (at least Silverblue + one other atomic variant) pass locally
3. Manual QA on real Silverblue hardware using
   [docs/testing/user-stories.md](../testing/user-stories.md) as the
   checklist — all 10 stories including destructive ones
4. Record the demo video on real hardware (issue #32)

## Troubleshooting

### `quickget fedora 42 silverblue` fails

Check the [quickemu wiki](https://github.com/quickemu-project/quickemu/wiki)
for current supported editions. Older or newer Silverblue releases may
also be available; adjust `LACS_VM_RELEASE`.

### VM boots but `silverblue-vm.sh ssh` times out

The Fedora installer doesn't enable sshd by default. Either enable
`sshd` during the interactive install, or boot the VM's GUI console
once to run:

```sh
sudo systemctl enable --now sshd
```

### `provision` step fails during rpm-ostree install

rpm-ostree install requires a reboot. The provision script auto-reboots
and asks you to re-run it. If it got stuck, just run `provision` again —
it's idempotent.

### Ollama model pulls too slowly

The provisioner defaults to `qwen3:0.6b` (~500 MB). Override with:

```sh
LACS_TEST_MODEL=gemma3:1b ./tests/e2e/silverblue-vm.sh provision
LACS_TEST_MODEL=qwen3:8b  ./tests/e2e/silverblue-vm.sh provision   # slow on CPU
```

Larger models give more reliable planning but take longer to load and
run on CPU. For daily testing, `qwen3:0.6b` is fast enough.

### CPU-only inference is too slow

Stories take 10-30 seconds each instead of 1-3 seconds with GPU.
GPU passthrough to QEMU/KVM is possible but requires VFIO setup, which
is out of scope for this guide.

### Stories fail with "daemon socket not found"

Check the daemon inside the VM:

```sh
./tests/e2e/silverblue-vm.sh ssh -- sudo systemctl status lacs-daemon
./tests/e2e/silverblue-vm.sh ssh -- sudo journalctl -u lacs-daemon -n 100
```

The provision log at `/var/log/lacs-e2e-provision.log` usually has the
root cause.

### Getting help

- Check [existing issues](https://github.com/lacs-foundation/lacs/issues)
- Open a new issue with:
  - The failing story log (`tests/e2e/logs/story-N.log`)
  - The daemon journal: `sudo journalctl -u lacs-daemon -n 200`
  - Your Fedora variant and release (from `rpm-ostree status`)
