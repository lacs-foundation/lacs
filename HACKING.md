# Hacking on LACS

Real-world notes for getting a working LACS development + testing setup
on your own hardware, with all the gotchas we hit and the workarounds
that stuck. If you just want to run unit tests, see
[CONTRIBUTING.md](CONTRIBUTING.md) — `cargo test --workspace` and `pnpm
test` are the only required gates for a PR.

If you want to run LACS end-to-end against a real Fedora Atomic Desktop
in a VM — for story validation, demo recording, or pre-release QA —
keep reading.

## TL;DR

```sh
# One-time host setup (Ubuntu 24.04 shown — adjust for your distro)
sudo add-apt-repository -y ppa:flexiondotorg/quickemu
sudo apt-get update
sudo apt-get install -y quickemu qemu-system-x86 qemu-utils \
    qemu-system-modules-spice rsync netcat-openbsd libguestfs-tools
sudo chmod +r /boot/vmlinuz-*            # required for libguestfs, see §2
test -r /dev/kvm || sudo usermod -aG kvm "$USER"   # then log out / back in

# LACS VM lifecycle
./tests/e2e/silverblue-vm.sh keygen      # dedicated SSH key (no passphrase)
./tests/e2e/silverblue-vm.sh download    # fetch Silverblue ISO (~2.7 GB)
./tests/e2e/silverblue-vm.sh install     # run Anaconda — just click through
./tests/e2e/silverblue-vm.sh bootstrap   # offline patch: user/passwords/sshd/key
./tests/e2e/silverblue-vm.sh start       # headless boot with SSH forward
./tests/e2e/silverblue-vm.sh provision   # build + install LACS + pull model
./tests/e2e/silverblue-vm.sh stop
./tests/e2e/silverblue-vm.sh snapshot baseline    # snapshot before tests
./tests/e2e/silverblue-vm.sh start
./tests/e2e/silverblue-vm.sh run         # stories 1–7
LACS_ALLOW_DESTRUCTIVE=1 ./tests/e2e/silverblue-vm.sh run  # all 10
./tests/e2e/silverblue-vm.sh stop
./tests/e2e/silverblue-vm.sh restore baseline
```

Everything below is the _why_ behind each step — read when something
breaks.

---

## 1. Why a VM, not just unit tests

LACS is three programs talking over a Unix socket:

- `lacs-brain` (LLM planner)
- `lacs-daemon` (privileged executor)
- `lacs-shell` (Tauri/React UI)

Unit tests cover the parsers, the safety fence, the IPC framing, the
reducer transitions — the code-shaped bugs. They do **not** cover:

- Is the daemon's systemd unit wired correctly against a real
  rpm-ostree host?
- Does `rpm-ostree install <pkg>` actually work when driven through
  the daemon's action executor?
- Do the preview/approval hashes survive a reboot?
- Does the shell render correctly against a real daemon over a real
  Unix socket?

A VM is the cheapest way to answer those questions without destroying
your workstation. LACS is designed to run on Fedora Atomic Desktops
(Silverblue, Kinoite, Sericea, Onyx), so the VM has to be one of those
— testing on plain Fedora Cloud doesn't exercise rpm-ostree's
deployment model.

We use [quickemu] for the VM because it automates:

- downloading the official Silverblue ISO,
- generating a working QEMU/KVM configuration,
- forwarding an SSH port from host to guest.

[quickemu]: https://github.com/quickemu-project/quickemu

## 2. Host prerequisites — the non-obvious ones

### KVM access

QEMU needs `/dev/kvm` readable by your user. On most distros this is
handled by the `kvm` group, but Ubuntu 24.04 also adds an ACL so
`test -r /dev/kvm` may already return true even if you're not in the
group. Check:

```sh
test -r /dev/kvm && echo "KVM OK" || echo "need kvm group"
```

If you need to add yourself: `sudo usermod -aG kvm "$USER"` and log out
and back in.

### quickemu version

Ubuntu 24.04's default `quickemu` package is too old — it downloads
`.ociarchive` files for Fedora Atomic editions instead of the proper
`.iso` (the fix landed in upstream PR #1503 in Nov 2024). Use the
`flexiondotorg/quickemu` PPA:

```sh
sudo add-apt-repository -y ppa:flexiondotorg/quickemu
sudo apt-get update
sudo apt-get install -y quickemu
```

On Fedora 41+ the default repo is current.

### libguestfs + the Ubuntu kernel-readable fix

`virt-customize` / `guestfish` run an in-memory "appliance" built from
your host kernel. Ubuntu 24.04 ships `/boot/vmlinuz-*` as mode `0600
root:root`, so libguestfs cannot read it as your user and fails with a
confusing `supermin exited with error status 1` error. One-time fix:

```sh
sudo chmod +r /boot/vmlinuz-*
```

You only need to do this once. Fedora / Arch / openSUSE ship kernels
world-readable by default.

## 3. The install — what Anaconda gets wrong

When you run `./tests/e2e/silverblue-vm.sh install`, a QEMU window
opens with the Fedora installer (Anaconda). You're tempted to think you
need to click through User Creation + set a password.

You don't. Our workflow deliberately **patches the installed disk
offline** using `libguestfs` instead, because:

- Anaconda on Fedora 42 silently skips User Creation if you don't
  explicitly click the Done button twice after setting a weak password.
  We found this the hard way — the VM booted with no login.
- `gnome-initial-setup` (the "welcome" wizard on first graphical boot)
  crashes on Fedora 42's third-party-repo toggle screen.
- `virtio-vga-gl` (virgl) flickers and freezes on hybrid-graphics hosts
  (Intel iGPU + NVIDIA dGPU).

The quickemu conf we auto-append sets `gl="off"` to avoid the virgl
crash, but the other two problems would still block us if we relied on
the graphical first-boot.

Instead, just click through Anaconda with default answers (Partitioning
→ Done, Begin Installation, wait, close the window when you see the
"Complete!" screen). **Don't click "Reboot"** — it will re-mount the
ISO as a CD-ROM and start the installer over. Close the window
instead.

Then run `./tests/e2e/silverblue-vm.sh bootstrap`, which via `guestfish`:

- Creates the `lacsdev` user (uid 1000, home `/home/lacsdev`, wheel
  group).
- Sets `lacsdev:lacsdev` and `root:lacs` passwords (SHA-512 via
  `openssl passwd -6`).
- Adds a NOPASSWD sudoers fragment so `sudo` works without a password
  (required for the automated provisioner).
- Enables `sshd` by dropping a systemd preset symlink — **Silverblue
  ships sshd disabled by default** on its workstation-class variants.
- Installs your passphrase-less SSH key at
  `/home/lacsdev/.ssh/authorized_keys`.
- Flips SELinux to permissive — we edit `/etc/shadow` and other files
  via guestfish without the correct SELinux labels, which causes sshd
  to reject key authentication in enforcing mode. We don't test
  SELinux semantics, so permissive is fine.
- Pre-marks `gnome-initial-setup` as done so it doesn't run.

All of that happens while the VM is **stopped** — no interaction
needed, no flaky GUI timing.

## 4. Why a dedicated SSH key

Contributors' personal `~/.ssh/id_ed25519` keys are typically
passphrase-protected. Interactive `ssh` works because `ssh-agent`
caches the unlocked key, but `rsync` (via `BatchMode=yes`) cannot
prompt for a passphrase and silently falls through to password auth
that we don't allow. End result: `rsync: connection closed` that takes
twenty minutes of debugging to diagnose.

`./silverblue-vm.sh keygen` generates a dedicated `~/.ssh/lacs-vm` key
with **no passphrase**. This is safe because the key only authorizes
login to your disposable test VM.

## 5. The `/usr` is read-only, but our Makefile wrote there

Fedora Atomic Desktops keep `/usr` as a read-only overlay managed by
rpm-ostree. The LACS Makefile defaults write to `/usr/lib/sysusers.d/`,
`/usr/lib/tmpfiles.d/`, `/usr/lib/systemd/system/`, and
`/usr/share/polkit-1/rules.d/` — all of which fail with "Read-only
file system" on Silverblue.

Fix: every path in the Makefile is now override-able with `?=`, and
the provisioner auto-detects rpm-ostree and passes the correct `/etc`
overrides:

```sh
sudo make install \
    SYSUSERS=/etc/sysusers.d \
    TMPFILES=/etc/tmpfiles.d \
    SYSTEMD=/etc/systemd/system \
    POLKIT=/etc/polkit-1/rules.d
```

systemd looks at `/etc/systemd/system/` **first** anyway (it wins over
`/usr/lib/systemd/system/`), so this is conceptually the right place
for a locally-built package. On non-ostree systems the default
Makefile behaviour still works unchanged.

## 6. The provisioner is two-phase, on purpose

`rpm-ostree install` can't add packages to a running deployment — it
stages them into the next deployment and requires a reboot to activate.
So `tests/e2e/provision.sh` splits into:

1. **Phase 1:** `rpm-ostree install gcc gcc-c++ make openssl-devel
   pkg-config zstd` → `systemctl reboot`. (Yes, `zstd` is needed: the
   Ollama installer unpacks its tarball with `unzstd`.)
2. **Phase 2 (after reboot):** rustup install, Ollama install
   (self-healing systemd unit if the upstream installer was
   interrupted), model pull, cargo build, `make install` with `/etc/`
   overrides, start lacs-daemon.

`./silverblue-vm.sh provision` detects which phase to run via a marker
file (`/var/lib/lacs-e2e/layered`). You'll need to run it **twice** the
first time — the script will tell you when to re-run after the
auto-reboot.

## 7. Ollama's installer is fragile

The official `curl -fsSL https://ollama.com/install.sh | sh` does
multiple things:

- Downloads `ollama-linux-amd64.tar.zst` from a CDN that can be very
  slow (30 KB/s on some networks, 10 MB/s on others).
- Unpacks it to `/usr/local/bin/ollama` (works on Silverblue — that
  path is writable).
- Tries to create a system `ollama` user + `/etc/systemd/system/
  ollama.service`. On Silverblue this can half-fail silently.
- If interrupted mid-download (e.g. you Ctrl-C because the CDN is
  stuck), you end up with a binary but no unit, no user, no way to run
  `ollama serve` as a service.

Our provisioner detects the missing unit and writes a minimal one
itself:

```ini
[Unit]
Description=Ollama Service
After=network-online.target

[Service]
ExecStart=/usr/local/bin/ollama serve
Environment=HOME=/var/lib/ollama
Environment=OLLAMA_HOST=127.0.0.1:11434
Restart=always
User=lacsdev
Group=lacsdev

[Install]
WantedBy=default.target
```

This runs Ollama as `lacsdev` with `~/.ollama` redirected to
`/var/lib/ollama`, which we pre-create with correct ownership.

## 8. Choosing a model — or, "why is the LLM so slow"

The LLM runs **inside the VM, on CPU**, unless you've set up GPU
passthrough (which is out of scope for this guide — VFIO + IOMMU is a
separate rabbit hole). That gives you:

| Model | Size | Tool calling | CPU-only speed | Use case |
|---|---|---|---|---|
| `qwen3:0.6b` | 500 MB | unreliable | fast | never — too small to emit correct tool calls |
| `qwen3:1.7b` | 1 GB | unreliable | fast | still too small for structured output |
| `qwen3:4b` | 2.5 GB | marginal | medium | simple plans only |
| **`qwen3:8b`** | **5 GB** | **reliable** | **20–45 s/story** | **default — sweet spot for CPU-only** |
| `qwen3:14b` | 9 GB | very reliable | 3–10 min/story | impractical without GPU |
| `qwen3:30b-a3b` | 18 GB | excellent | depends | MoE — only 3B active params, fast-ish |

Qwen3 also has a "thinking mode" that emits thousands of preamble
tokens before the real answer. On CPU this explodes the latency. You
can disable it inline by prepending `/nothink` to the prompt, or
globally via the Modelfile's `/set nothink`, but our current provisioner
doesn't do either — if speed matters, use `qwen3:8b` where even with
thinking on, a plan finishes in about a minute.

Override the default via `LACS_TEST_MODEL`:

```sh
LACS_TEST_MODEL=qwen3:8b  ./tests/e2e/silverblue-vm.sh provision
LACS_TEST_MODEL=qwen3:14b ./tests/e2e/silverblue-vm.sh provision   # needs GPU
```

## 9. Env vars don't magically cross SSH

This one bit us hard. When you run `./silverblue-vm.sh run`, the
wrapper does:

```
ssh lacsdev@localhost  →  sudo -E  →  bash run-stories.sh
```

SSH by default only forwards `TERM` / `LANG` / `LC_*`. Any `LACS_*` env
var you set on the host **does not** reach the test CLI in the guest
unless we forward it explicitly. `sudo -E` alone isn't enough either —
sudoers' `env_reset` default filters unknown variables.

The fix in `cmd_run` builds a `VAR='val' ...` prefix and passes it to
sudo directly: `sudo VAR=val bash run-stories.sh`. That injects the
value into the child's env regardless of sudoers config.

Forwarded vars: `LACS_ALLOW_DESTRUCTIVE`, `LACS_LLM_PROVIDER`,
`LACS_LLM_MODEL`, `LACS_TEST_MODEL`, `LACS_OLLAMA_URL`,
`LACS_LISTEN_URI`, `LACS_STORY_TIMEOUT`.

## 10. Common gotchas, in the order we hit them

1. **`quickget` edition names are capitalized**: `Silverblue`,
   `Kinoite`, `Sericea` (Sway), `Onyx` (Budgie). The script maps
   lowercase input to the correct case.
2. **`quickget` outputs to `fedora-<release>-<Edition>/`, uppercase**.
   Early versions of our script looked in the lowercase path and
   couldn't find anything.
3. **Ollama downloads `.ociarchive` instead of `.iso` on old
   quickemu.** Use the flexiondotorg PPA.
4. **gnome-initial-setup crashes on Fedora 42.** `virgl=off` and
   skipping the wizard via `bootstrap` both help.
5. **Anaconda silently skips User Creation on weak passwords.**
   Bootstrap creates the user offline instead.
6. **SELinux rejects our offline-edited `~/.ssh/authorized_keys`.**
   We set SELinux to permissive in `bootstrap`.
7. **Personal SSH keys are passphrase-protected.** `keygen` creates a
   dedicated passphrase-less key.
8. **Ollama install.sh needs `zstd`.** Layered in phase 1 of provisioner.
9. **Ollama systemd unit may be missing on Silverblue.** Self-healing
   fallback in phase 2.
10. **`make install` writes to read-only `/usr`.** Makefile paths are
    now overridable; provisioner auto-uses `/etc/` on ostree.
11. **Env vars don't cross SSH.** `cmd_run` forwards them through sudo.
12. **Qwen3's thinking mode + CPU = minutes per story.** Use a smaller
    model, prepend `/nothink`, or enable GPU passthrough.

## 11. Snapshots are your friend

First-time provisioning takes 30–60 minutes (mostly waiting for the
Ollama CDN and the Rust release build). Once the VM is provisioned and
working:

```sh
./tests/e2e/silverblue-vm.sh stop
./tests/e2e/silverblue-vm.sh snapshot baseline
```

Every subsequent run becomes:

```sh
./tests/e2e/silverblue-vm.sh start
./tests/e2e/silverblue-vm.sh run
./tests/e2e/silverblue-vm.sh stop
./tests/e2e/silverblue-vm.sh restore baseline
```

No more waiting on Ollama, Rust, or Anaconda. The baseline is a
qcow2 internal snapshot — no extra disk space until you diverge.

## 12. When to reach for this VM vs. just `cargo test`

| Change you're making | Good enough gate |
|---|---|
| Rust logic / parsers / reducers | `cargo test --workspace` |
| React components | `pnpm test` |
| IPC wire format | `cargo test --workspace` + PR with `e2e` label (CI smoke) |
| Action catalogue (new actions) | Unit tests + VM run, stories 1–7 |
| rpm-ostree / systemd integration | **VM required** |
| Packaging / Makefile / sudoers / polkit | **VM required** |
| Release candidates | **VM + manual demo recording on real hardware** |

## 13. Cleaning up

- `./tests/e2e/silverblue-vm.sh destroy` removes the VM disk but keeps
  the downloaded ISO under `tests/e2e/vm/`. That directory is
  gitignored; feel free to delete it entirely when you're done.
- The dedicated VM SSH key at `~/.ssh/lacs-vm` is harmless to leave
  around; you can `rm ~/.ssh/lacs-vm*` if you want.

## 14. See also

- [`docs/contributing/testing.md`](docs/contributing/testing.md) — the
  short reference version of this file.
- [`docs/testing/user-stories.md`](docs/testing/user-stories.md) — the
  10 stories the harness runs and their pass criteria.
- [`tests/e2e/silverblue-vm.sh help`](tests/e2e/silverblue-vm.sh) — the
  subcommand reference, kept in sync with the script itself.
