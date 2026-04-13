# Testing LACS

This guide explains how to run LACS tests at every level — from the
fast unit tests you run locally on every change to the VM-based end-to-end
validation before a release.

## Test pyramid

| Level | What it tests | Speed | Run on |
|---|---|---|---|
| Unit tests (Rust) | Individual functions, parsers, traits | <5s | Every commit, every CI run |
| Unit tests (TypeScript) | React components, reducers, IPC shims | <5s | Every commit, every CI run |
| Integration (Rust) | Daemon IPC, safety fence, policy | <10s | Every commit, every CI run |
| E2E CI smoke | Daemon + Ollama + read-only stories in a container | 5-10 min | Opt-in (PR label `e2e` or manual trigger) |
| E2E Vagrant | Full VM with systemd, real Ollama model | 10-30 min | Local dev / self-hosted / pre-release |
| Manual QA | Real Silverblue/Kinoite hardware, destructive actions, GUI | 30-60 min | Before releases |

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

## Running the E2E CI smoke test (opt-in)

The smoke test boots Ollama and the daemon directly in a GitHub Actions
runner (no VM), pulls a small tool-capable model (`gemma3:1b`), and runs
the 7 read-only user stories.

**Trigger it one of two ways:**

1. Label a PR with `e2e` — the workflow runs automatically
2. Manual dispatch via the GitHub Actions UI (Actions → e2e → Run workflow)

Results appear as the `container-smoke` job in the `e2e` workflow. Story
logs are uploaded as a build artifact so you can inspect individual
plan outputs.

The smoke test cannot cover:

- rpm-ostree actions (no ostree deployment in the runner)
- GUI rendering (no display server)
- Reboot / kernel-argument flows

## Running the full VM test suite (local)

For a higher-fidelity test you can boot a Fedora VM on your own hardware
via Vagrant. This works on Linux, macOS, and Windows.

### Prerequisites

**Linux (recommended):**

```sh
# Fedora/RHEL/atomic
sudo dnf install vagrant vagrant-libvirt libvirt-daemon-system qemu-kvm

# Ubuntu/Debian
sudo apt install vagrant vagrant-libvirt libvirt-daemon-system qemu-kvm
```

**Linux (VirtualBox fallback) or macOS / Windows:**

1. Install [VirtualBox 7+](https://www.virtualbox.org/wiki/Downloads)
2. Install [Vagrant](https://developer.hashicorp.com/vagrant/downloads)

### Boot the VM and run the stories

```sh
# From the repo root. Uses libvirt by default; set VAGRANT_PROVIDER to switch.
vagrant up

# Or explicitly:
VAGRANT_PROVIDER=virtualbox vagrant up

# Read-only stories (1-7)
vagrant ssh -c 'cd /vagrant && sudo -E tests/e2e/run-stories.sh'

# All stories including destructive ones (requires a VM snapshot if you
# want to revert changes). Stories 8-10 WILL modify the VM state.
vagrant ssh -c 'cd /vagrant && sudo -E LACS_ALLOW_DESTRUCTIVE=1 tests/e2e/run-stories.sh'

# Shutdown (keeps the VM) / destroy (removes the VM)
vagrant halt
vagrant destroy -f
```

### What the VM includes

The provisioner script (`tests/e2e/provision.sh`) installs:

- Rust stable (for building the daemon)
- Ollama + a small model (`qwen3:0.6b` by default; override with
  `LACS_E2E_MODEL` env var)
- firewalld, podman, toolbox, flatpak (for the query tools)
- A test user `lacsdev` with a pre-seeded SSH public key
- The LACS daemon running as a systemd service

### Important: not a faithful Silverblue

The Vagrantfile uses `fedora/42-cloud-base` as the base image because
official Vagrant boxes for Silverblue/Kinoite are sparse and outdated.
The cloud base image has the same rpm-ostree tooling installed, but it
is **not** actually deployed via ostree. This means:

| What the VM CAN test | What it CAN'T test |
|---|---|
| Read-only query tools | `rpm-ostree install` mutations |
| IPC and safety fence | `rpm-ostree rebase` / upgrade flows |
| Policy and role checks | `RebootSystem` (meaningless in a transient VM) |
| Plan generation via Ollama | Automatic rollback after ostree failure |
| SSH key management | Tauri GUI rendering |
| Systemd service queries | Real kernel-argument changes |

For full Silverblue-specific testing, install Silverblue in a VM from
the official ISO. Tools like [quickemu] make this a 2-command install:

```sh
quickget fedora 42 silverblue
quickemu --vm fedora-42-silverblue.conf
```

[quickemu]: https://github.com/quickemu-project/quickemu

Once the Silverblue VM is running, follow the same provisioning steps
manually (`tests/e2e/provision.sh` is portable) or adapt the `Vagrantfile`.

## Running individual stories

Each of the 10 stories is a self-contained shell script:

```sh
# Inside the VM or on a provisioned machine
cd /vagrant  # or wherever the repo lives

# Run a specific story by number
sudo -E tests/e2e/run-stories.sh 3

# Run multiple specific stories
sudo -E tests/e2e/run-stories.sh 1 4 7
```

Per-story logs are written to `tests/e2e/logs/story-N.log`.

## Before opening a PR

1. Run `cargo test --workspace` and `pnpm test` — these must pass
2. Run `cargo clippy --workspace --all-features --locked -- -D warnings`
3. Run `cargo fmt --all --check`
4. For substantial changes to the brain/daemon/IPC layer, also run
   the VM tests locally: `vagrant up && vagrant ssh -c 'cd /vagrant && sudo -E tests/e2e/run-stories.sh'`
5. If your PR touches action definitions, policy, or the planning loop,
   add the `e2e` label to trigger the CI smoke test

## Before a release

The maintainer runs these in order:

1. All automated tests green on main
2. VM tests (both providers) pass locally
3. Manual QA on real Silverblue hardware using
   [docs/testing/user-stories.md](../testing/user-stories.md) as the
   checklist — all 10 stories including destructive ones
4. Record the demo video (#32) on real hardware

## Troubleshooting

### `vagrant up` fails with `libvirt: no bridge interface`

Install `vagrant-libvirt`'s required packages:

```sh
sudo dnf install libvirt-daemon-kvm libvirt-daemon-driver-qemu
sudo systemctl enable --now libvirtd
sudo usermod -aG libvirt $USER
# Log out and back in
```

### Ollama model pulls too slowly in the VM

The provisioner defaults to `qwen3:0.6b` (~500 MB). For faster setup:

```sh
LACS_E2E_MODEL=qwen3:0.6b vagrant provision   # tiny, less reliable planning
LACS_E2E_MODEL=gemma3:1b  vagrant provision   # ~700 MB, better planning
LACS_E2E_MODEL=qwen3:8b   vagrant provision   # production default; slow on CPU
```

### CPU-only inference is too slow

If you have an NVIDIA GPU and are using libvirt, add GPU passthrough to
the Vagrantfile. For most testing, CPU is fast enough — stories take
10-30 seconds each instead of 1-3 seconds with GPU.

### Stories fail with "daemon socket not found"

Check the daemon status inside the VM:

```sh
vagrant ssh
sudo systemctl status lacs-daemon
sudo journalctl -u lacs-daemon -n 50
```

If the daemon didn't start, the provisioner log at
`/var/log/lacs-provision.log` usually has the cause.

### Getting help

- Check [existing issues](https://github.com/lacs-foundation/lacs/issues)
- Open a new issue with the failing story log (`tests/e2e/logs/story-N.log`)
  and the daemon journal output
