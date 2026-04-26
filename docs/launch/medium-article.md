# Medium Article

> Publication target: Towards Dev or plain personal Medium
> Canonical URL: dev.to article (set canonical to avoid duplicate-content penalty)
> Estimated read time: 10–12 min

---

# The day I stopped letting AI paste commands into my terminal

It started with a perfectly reasonable suggestion.

I was troubleshooting a Podman networking issue on my Silverblue box — one of
those evenings where you have three browser tabs open, a failing container, and
no patience. I asked an AI assistant for help. It responded with a shell
one-liner. I copied it. I pasted it. It ran.

The container came up. Problem solved.

But as I closed the terminal I noticed something unsettling: I had no idea what
had actually happened. The command had run. My system had changed. I had
no record of what changed or why, no way to reverse it if something went wrong
later, and no evidence that what the AI suggested was the minimal change rather
than a sledgehammer.

For a homelab that is probably fine. For a machine you depend on — or for a
team that has to answer audit questions — it is not.

That evening I started building SysKnife.

## What I was trying to fix

The AI terminal tools that existed at the time (and the ones that have launched
since) share a common pattern: the model produces a string, the string goes to
a shell. The variation is in the confirmation step — some tools ask you to press
Enter, some add a "are you sure?" prompt, some skip the step entirely.

But the fundamental unit is still a string. A string that a human has to read,
evaluate, and decide whether to run. Which is basically what I was doing when
I pasted that Podman one-liner by hand.

I wanted something with a harder guarantee. Not "the model wrote something that
looks reasonable" but "the model wrote something from a list of things that
are structurally valid and the system knows how to reverse."

## The typed action model

SysKnife gives the LLM a typed API instead of a shell.

The planner — an LLM running locally via Ollama or via any cloud provider you
choose — can propose actions from a fixed vocabulary. Each action is a named
variant in a Rust enum. There are currently 80+ variants covering packages,
services, firewall rules, containers, Flatpak apps, toolboxes, SSH keys, kernel
arguments, user management, and deployment pinning.

When you type a request, the planner produces a plan: an ordered list of typed
actions with risk levels. You see each step before anything runs:

```
$ sysknife "install git and enable sshd"

Plan (2 steps):
  1. InstallPackage  [Low]   rpm-ostree install git
  2. EnableService   [Low]   systemctl enable --now sshd

Approve step 1? [y/N] y
  ✓ Done. Audit entry #49.

Approve step 2? [y/N] y
  ✓ Done. Audit entry #50.
```

If you say no to any step, the remaining steps do not run. If a high-risk step
fails, the daemon runs the inverse action automatically before surfacing the
error. No partial state left behind.

## The piece that matters: where the privilege lives

This was the design decision that took the longest to get right.

My first version had the approval step in the planner. The planner would
produce a plan, ask for approval, then execute. It seemed natural — the planner
was the component talking to the user.

The problem is that the planner is an unprivileged process that talks to an
LLM provider over the network. If the planner also held execution rights, a
sufficiently crafted LLM response could potentially manipulate the approval
logic. The trust boundary was in the wrong place.

In SysKnife the trust boundary is between the shell (the approval gate) and the
daemon (the executor). The planner produces a plan and hands it to the shell.
The shell shows you the plan and collects your approval. The daemon — the only
privileged process in the system — executes only what the shell passes with a
valid approval. The planner has no path to execution that does not go through
you.

The daemon is also the only component that touches the OS. The planner never
has a file descriptor that reaches kernel interfaces. The shell never has
elevated privileges. The boundary is mechanical — it does not depend on the
model being well-behaved.

## The audit trail I actually wanted

SQLite rows are easy to write and easy to delete. A log you can tamper with is
not an audit trail — it is a suggestion.

Every SysKnife execution appends to a HMAC-SHA256 hash chain. Each entry
includes a hash of its own content plus the hash of the previous entry. If you
delete a row, modify a result, or reorder two entries, the next `sysknife audit
verify` run fails immediately with the index of the first broken link.

The chain also carries an epoch watermark. Rolling back the system clock does
not let you insert fake entries with backdated timestamps.

For a homelab this is mostly peace of mind — a year from now I can look at the
log and know exactly what SysKnife did and verify the record is complete. For
a regulated environment it is more than that: RFC 5424 syslog forwarding to
Splunk, Microsoft Sentinel, or QRadar ships out of the box, and a Postgres
backend is available for teams that need a central, queryable audit store with
cloud database support.

## Running it offline

One of the things I care about most: SysKnife works fully offline with Ollama.

```toml
# ~/.config/sysknife/config.toml
[llm]
provider   = "ollama"
model      = "qwen3:8b"
ollama_url = "http://localhost:11434"
```

qwen3:8b runs comfortably on a laptop with 16 GB of RAM. No API key. No
network call. The planning, approval, execution, and audit all happen locally.
For homelab users who are privacy-conscious — and the r/selfhosted community
survey suggests most of them are — this is the configuration I recommend.

## What it looks like in practice

I have been running SysKnife on my Silverblue daily driver for several months
now. The interactions I reach for most:

**Checking state** (no approval needed, read-only):

```text
sysknife "show me disk usage and which services have failed recently"
```

The planner produces a `GetDiskUsage` + `ListFailedServices` plan; the daemon
executes both read-only queries and returns the output. No approval gate for
read-only actions by default — you can configure the risk ceiling.

**Package management:**

```text
sysknife "install neovim"
```

On Silverblue the planner correctly calls `rpm-ostree install neovim` rather
than `dnf install` — it knows about the immutable OS model. I tested this
specifically after a previous AI assistant had suggested `dnf` three times in
a row on the same box.

**Deployment pinning:**

```text
sysknife "did my last deployment succeed? pin it if it did."
```

The planner calls `query_job_history` to check SysKnife's own transaction
log, then proposes `PinDeployment` if the last deployment succeeded. This is
exactly the kind of two-step "check state, then act" workflow that typed
actions handle well.

## What is not finished

I want to be direct about the current scope because overclaiming is the fastest
way to lose trust with the people who matter most.

Fedora 41+ and Silverblue 41+ are fully supported. Ubuntu action families are
implemented in the codebase — the work is real — but the planner-side distro
detection hints and live end-to-end validation against a Ubuntu VM are still in
progress. I would not run SysKnife in production on Ubuntu today.

Arch, NixOS, and other distros are roadmap items. The LACS protocol — the
typed action specification that SysKnife implements — is CC0 and lives at
github.com/lacs-foundation/specification. Other implementations for other
distros are explicitly encouraged. The spec is the thing I want to outlast any
single implementation.

## The part where I ask you to try it

```sh
# Silverblue / Fedora 41+
git clone https://github.com/lacs-foundation/sysknife
cd sysknife && make build && sudo make install
sudo systemctl enable --now sysknife-daemon
sysknife --dry-run "show disk usage"

# Offline, with Ollama
ollama pull qwen3:8b
# set provider = "ollama" in ~/.config/sysknife/config.toml
sysknife "show disk usage"
```

If you find something that does not work — an action that fails silently, a
planner output that surprises you, a distro edge case I missed — I want to
hear about it. Open an issue or come to GitHub Discussions.

The audit chain only means something if the thing it is auditing is reliable.

---

- GitHub: <https://github.com/lacs-foundation/sysknife>
- Docs: <https://lacs-foundation.github.io/sysknife/>
- LACS spec: <https://github.com/lacs-foundation/specification>
- AGPL-3.0 license.
