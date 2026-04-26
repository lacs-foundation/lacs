# r/linux — Thread Draft

## Title

**I built SysKnife: an AI Linux sysadmin where the AI plans but cannot run
shell. Typed actions, approval gate, tamper-evident audit chain. AGPL-3.0,
Fedora-first.**

---

## Body

I have been running Silverblue as my daily driver for about two years. The
thing I kept running into: I would ask an AI assistant for help with a task,
get a shell one-liner, paste it, and have no idea what just happened to my
system. If it went wrong I had no record of what was attempted, no rollback,
and no way to verify my system state had not been silently modified.

SysKnife is my attempt to fix that.

![SysKnife demo](https://raw.githubusercontent.com/lacs-foundation/sysknife/main/assets/demo/demo.gif)

Here is what a session looks like:

```
$ sysknife "add a firewall rule to block port 8080 from outside"

Plan (1 step):
  1. AddFirewallRule  [Medium]  firewall-cmd --add-rich-rule \
                                 'rule family="ipv4" port port="8080" protocol="tcp" drop'
     Side effects: firewall rule persisted across reboots
     Rollback: RemoveFirewallRule (automatic on failure)

Approve step 1? [y/N] y
✓ Done. Audit entry #48 written.
```

The brain (an LLM — Ollama works, so does any cloud provider) proposes a plan
as a list of typed actions. The daemon — a separate privileged process — only
executes what you approve explicitly. No shell string crosses the trust
boundary. Every action is a typed Rust enum validated at compile time.

**Why typed actions instead of shell strings?**

Shell strings are untyped. "Run `firewall-cmd …`" and "run `rm -rf /`" are the
same type at the LLM layer. Typed actions have a finite set of valid variants.
The daemon rejects anything outside the set at the enum match — no parsing, no
injection surface.

**The audit chain**

Every execution appends a row to a SQLite table. Each row carries the HMAC-SHA256
of its content plus the previous row's hash. `sysknife audit verify` walks the
chain: any deleted or modified row breaks verification immediately.

**The rollback**

High-risk actions that fail trigger automatic rollback — `rpm-ostree rollback`
for deployment changes, inverse firewall rules for firewall changes, etc.

**Current state**

- Fedora 41+ and Silverblue 41+: fully supported.
- Ubuntu action families: landed in the codebase; planner hints and live E2E
  validation on Ubuntu VM are in flight.
- 1 026 tests passing on every commit.
- 80+ typed actions: packages, services, firewall, containers, flatpak,
  toolbox, SSH keys, kernel args, users, deployments.
- AGPL-3.0 license.
- Works offline with Ollama (qwen3:8b runs well on a mid-range laptop).

**Links**

- Repo: <https://github.com/lacs-foundation/sysknife>
- Docs: <https://lacs-foundation.github.io/sysknife/>
- The LACS spec (the protocol SysKnife implements) is CC0 at
  <https://github.com/lacs-foundation/specification>

Happy to answer questions about the design decisions, the distro matrix,
or the protocol spec.
