# Ubuntu 24.04 VM testing

This guide explains how to validate SysKnife user stories against a live
Ubuntu 24.04 (Noble) environment using QEMU/KVM.

The Ubuntu path uses `qemu-system-x86_64` directly with a cloud-init seed
ISO — no quickemu, no interactive installer, no GUI window. The base image
is an official Ubuntu cloud image; a writable qcow2 overlay sits on top so
the base image is never modified.

## Test pyramid position

| Level | Fedora path | Ubuntu path |
|---|---|---|
| Unit / integration | `cargo nextest run` | same |
| Dev stories (no VM) | `dev-stories.sh` | same |
| E2E VM | `atomic-vm.sh` + Silverblue | **`ubuntu-vm.sh`** + Ubuntu 24.04 |

## Host requirements

- `qemu-system-x86_64` and `qemu-utils` (`qemu-img`)
- `genisoimage` (to build the cloud-init seed ISO)
- KVM module loaded (`/dev/kvm` readable)
- `rsync`, `ssh`, `curl`, `netcat-openbsd`

```sh
sudo apt-get install -y \
    qemu-system-x86 qemu-utils genisoimage \
    rsync netcat-openbsd
# Make /dev/kvm accessible
sudo usermod -aG kvm "$USER"   # log out and back in
# or: sudo setfacl -m u:$USER:rw /dev/kvm
```

## One-time setup

```sh
# From the repo root.

# 1. Prepare the base image and overlay (downloads ~600 MB on first run).
./tests/e2e/ubuntu-vm.sh download

# 2. Boot the VM once so cloud-init finishes first-boot provisioning
#    (installs tools, resizes rootfs, injects the SSH key).
#    The script polls SSH and returns when the VM is ready (~3-5 min).
./tests/e2e/ubuntu-vm.sh install
```

`download` and `install` are idempotent — safe to re-run.

## Daily use

```sh
# Boot the VM (skips cloud-init, boots in ~15 s)
./tests/e2e/ubuntu-vm.sh start

# SSH into the guest
./tests/e2e/ubuntu-vm.sh ssh

# Rsync the repo and run the full provisioner (builds + installs sysknife)
./tests/e2e/ubuntu-vm.sh provision

# Take a baseline snapshot after first provision
./tests/e2e/ubuntu-vm.sh stop
./tests/e2e/ubuntu-vm.sh snapshot baseline
./tests/e2e/ubuntu-vm.sh start

# Run the Ubuntu story suite
./tests/e2e/ubuntu-vm.sh run

# Roll back to the clean baseline
./tests/e2e/ubuntu-vm.sh stop
./tests/e2e/ubuntu-vm.sh restore baseline
```

## Configuration

All defaults live in `tests/e2e/ubuntu-vm.conf` and can be overridden with
environment variables:

| Variable | Default | Notes |
|---|---|---|
| `UBUNTU_VM_MEM` | `4096` | Guest RAM in MB |
| `UBUNTU_VM_CPUS` | `2` | Guest vCPUs |
| `UBUNTU_VM_DISK` | `20G` | qcow2 overlay size |
| `UBUNTU_VM_SSH_PORT` | `2223` | Host port → guest :22 |
| `UBUNTU_VM_USER` | `ubuntu` | Guest username |
| `UBUNTU_VM_IMAGE_CACHE` | `~/.cache/sysknife-vms` | Base image cache |
| `UBUNTU_VM_DIR` | `tests/e2e/ubuntu-vm` | Overlay + runtime files |

The `SYSKNIFE_VM_SSH_KEY` env var overrides the SSH key path (default:
`~/.ssh/sysknife-vm`, shared with `atomic-vm.sh`).

## Subcommand reference

```
./tests/e2e/ubuntu-vm.sh <subcommand> [args]

  download          Prepare base image + cloud-init seed ISO + qcow2 overlay
  install           First-boot: run cloud-init, wait for SSH (3-5 min)
  start             Boot overlay headlessly, wait for SSH (~15 s)
  stop              Graceful shutdown via SSH
  ssh [cmd]         Open a shell (or run cmd) inside the VM
  sync              Rsync the repo into /home/ubuntu/sysknife/
  provision         sync + run ubuntu-provision.sh as root
  run [N…]          Run run-stories.sh (optional: specific story numbers)
  snapshot <name>   Create a named internal qcow2 snapshot (VM stopped)
  restore  <name>   Restore a named snapshot (VM stopped)
  destroy           Delete the overlay (base image kept)
  help              Print subcommand list
```

## Running individual stories

```sh
./tests/e2e/ubuntu-vm.sh ssh
cd /home/ubuntu/sysknife
sudo -E tests/e2e/run-stories.sh 3        # story 3 only
sudo -E tests/e2e/run-stories.sh 1 4 7   # multiple stories
```

Logs land at `tests/e2e/logs/story-N.log` inside the VM.

## Differences from the Fedora Atomic (atomic-vm.sh) path

| Concern | Fedora (atomic-vm.sh) | Ubuntu (ubuntu-vm.sh) |
|---|---|---|
| Base image | Fedora Silverblue ISO | Ubuntu 24.04 cloud image |
| Boot tooling | quickemu | qemu-system-x86_64 direct |
| First-boot | Interactive Anaconda + guestfish offline patch | cloud-init (fully automated) |
| Package manager | rpm-ostree (layers + reboot) | apt-get (no reboot) |
| Provision phases | 2 (reboot between) | 1 |
| Firewall default | firewalld | ufw + firewalld |
| Container tooling | podman + toolbox (built in) | distrobox |

## Troubleshooting

### `download` says "partial download" and tries to re-download

The background download process may still be writing
`~/.cache/sysknife-vms/noble-server-cloudimg-amd64.img.tmp`. Wait for it
to finish, or remove the `.tmp` file and re-run `download` to fetch
directly.

### `install` times out waiting for SSH

Cloud-init may have encountered an error. Boot the VM in foreground mode to
watch the console:

```sh
# Edit ubuntu-vm.sh temporarily: replace _qemu_start yes → _qemu_start no
# then re-run install.
```

Check `/var/log/cloud-init-output.log` inside the VM.

### SSH succeeds but `provision` fails at `cargo build`

Rust may not be installed yet (e.g. `rustup` failed silently). Check:

```sh
./tests/e2e/ubuntu-vm.sh ssh 'source ~/.cargo/env && cargo --version'
```

If missing, install manually:

```sh
./tests/e2e/ubuntu-vm.sh ssh \
  'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
```

### `sysknife-daemon` fails to start

Check the journal:

```sh
./tests/e2e/ubuntu-vm.sh ssh 'sudo journalctl -u sysknife-daemon -n 100'
```

Provision log is at `/var/log/sysknife-e2e-provision.log`.

### Port 2223 already in use

Change the host port:

```sh
UBUNTU_VM_SSH_PORT=2224 ./tests/e2e/ubuntu-vm.sh start
```

### Getting help

Open an issue with:

- The failing step log
- `./tests/e2e/ubuntu-vm.sh ssh 'sudo journalctl -u sysknife-daemon -n 200'`
- `lsb_release -a` from inside the VM
