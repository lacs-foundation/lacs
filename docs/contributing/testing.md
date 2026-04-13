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
test -r /dev/kvm \
    || sudo usermod -aG kvm "$USER"   # then log out and back in
                                       # (or use ACL: setfacl -m u:$USER:rw /dev/kvm)
```

You also need `libguestfs-tools` for the offline disk patches we apply
between Anaconda's install and the first SSH login (set passwords,
install our SSH key, enable sshd). Ubuntu 24.04 keeps kernel images at
mode 0600 by default, which prevents libguestfs from running
unprivileged — fix once with `sudo chmod +r /boot/vmlinuz-*`.

```sh
sudo apt-get install -y libguestfs-tools
sudo chmod +r /boot/vmlinuz-*
```

**One-time VM setup:**

```sh
# From the repo root.

# 1. Generate a passphrase-less SSH key dedicated to the VM (you do
#    not want to reuse your personal id_ed25519 — rsync/non-interactive
#    ssh cannot prompt for a passphrase). Idempotent.
./tests/e2e/silverblue-vm.sh keygen

# 2. Download the Silverblue 42 ISO (~2.5 GB, cached under tests/e2e/vm/).
./tests/e2e/silverblue-vm.sh download

# 3. Run the Fedora installer interactively (GUI window opens).
#    Just click through it — you don't need to set a password or create
#    a user; we patch all of that into the disk image afterwards via
#    libguestfs. Shut the VM down when the installer finishes (close
#    the QEMU window or pick "Power Off" in the post-install screen —
#    DO NOT click "Reboot": the ISO will re-mount as CD-ROM).
./tests/e2e/silverblue-vm.sh install

# 4. Patch the disk image with our test user, password, sudoers, sshd,
#    and SSH key. (Implemented via guestfish so it works offline,
#    bypassing Silverblue's interactive first-boot wizard which has
#    bugs in F42.)
./tests/e2e/silverblue-vm.sh install-key
```

> Why no `enable-ssh` step? Earlier versions of this script tried to
> boot the VM visibly so the contributor could enable sshd by hand.
> That ran into Fedora 42's gnome-initial-setup crashing on the
> third-party-repo screen with virgl/Wayland bugs. The current flow
> sidesteps the GUI entirely by configuring the VM offline via
> `libguestfs`. The `enable-ssh` subcommand is still there as a fallback
> if your Anaconda install did create a usable user.

> What `bootstrap` does, in one shot: create user `lacsdev`, set the
> password (`lacsdev`), set root password (`lacs`), install your VM
> SSH key, NOPASSWD-sudoers `lacsdev`, enable `sshd`, set SELinux to
> permissive, and pre-mark `gnome-initial-setup` as done. Idempotent —
> safe to re-run after `install`.

**Run the tests:**

```sh
# Boot the VM headlessly (in the background)
./tests/e2e/silverblue-vm.sh start

# First-ever provision: rsyncs the repo into the VM, layers build tools
# via rpm-ostree, reboots the VM, then runs again to build LACS and
# pull the Ollama model. Re-run after the auto-reboot (the script tells
# you when). Expect 30-60 minutes total on first run (mostly waiting
# for Ollama tarball + Rust deps download). ~2 minutes on subsequent
# provisions.
./tests/e2e/silverblue-vm.sh provision

# RECOMMENDED: take a "baseline" snapshot now, before any test run.
# Future test runs can `restore baseline` instead of re-provisioning.
./tests/e2e/silverblue-vm.sh stop
./tests/e2e/silverblue-vm.sh snapshot baseline
./tests/e2e/silverblue-vm.sh start

# Run the read-only stories (1-7)
./tests/e2e/silverblue-vm.sh run

# Run ALL stories including destructive (8-10) — the destructive ones
# layer packages, create toolboxes, etc. Restore the baseline afterwards.
LACS_ALLOW_DESTRUCTIVE=1 ./tests/e2e/silverblue-vm.sh run

# Roll back to the clean baseline so the next run is fast
./tests/e2e/silverblue-vm.sh stop
./tests/e2e/silverblue-vm.sh restore baseline
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

### Ollama download or model pull is very slow

Two distinct downloads can be slow:

1. **The Ollama tarball itself** (~1.5 GB, downloaded by `install.sh` from
   `ollama.com/download/ollama-linux-amd64.tgz`). On some networks /
   geos / times of day this CDN serves at <100 KB/s. There's no
   workaround inside the script — wait it out, or pre-stage the binary
   on the host and copy it in via SSH if you're going to re-provision
   often.

2. **The model pull** (~5 GB for the default `qwen3:8b`). Happens
   after Ollama is installed, via `ollama pull`. Goes through Ollama's
   registry (usually faster than the ollama.com CDN).

Override the model size with `LACS_TEST_MODEL`:

```sh
LACS_TEST_MODEL=qwen3:8b  ./tests/e2e/silverblue-vm.sh provision   # default
LACS_TEST_MODEL=qwen3:14b ./tests/e2e/silverblue-vm.sh provision   # needs GPU passthrough
LACS_TEST_MODEL=qwen3:30b-a3b ./tests/e2e/silverblue-vm.sh provision  # MoE, needs 16G VM
```

We default to **`qwen3:8b`** after empirical testing on CPU-only VMs:
it produces correct tool calls reliably at ~20-45 s/story. Smaller
models (qwen3:0.6b / qwen3:1.7b) emit garbled tool calls; larger
models (qwen3:14b) are minutes per story on CPU because Qwen3's
"thinking mode" generates thousands of preamble tokens.

For the full history of what we tried and why, see
[HACKING.md](../../HACKING.md) §8.

**Tip:** once provision succeeds end-to-end, immediately `stop` the VM
and `snapshot baseline`. Then every subsequent test cycle becomes
`restore baseline → start → run`, skipping all the slow downloads.

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
