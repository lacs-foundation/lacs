# dev.to Article

> Tags: rust, linux, opensource, sysadmin
> Series: (none)
> Cover image: assets/logo/sysknife.svg or the demo GIF first frame

---

# I built an AI sysadmin in Rust — and the AI is not allowed to run shell

The first time I used an AI assistant to help manage my Silverblue box I typed:

> "add a firewall rule to drop inbound connections on port 8080"

It gave me a `firewall-cmd` one-liner. I pasted it. It worked. Then I thought:
what would happen if I pasted the wrong thing? What would happen if the model
had a bad day and suggested `iptables -F`? What would I have to show for it
afterward — a vague memory of having run something in a terminal?

That question became SysKnife.

## The idea: typed actions, not shell strings

Every AI terminal tool I looked at shared one design: the LLM produces a
string, the string goes to a shell. The safety guarantee is soft — "the model
probably wrote something reasonable."

I wanted a harder guarantee. So I gave the LLM a typed API instead.

SysKnife exposes a finite set of actions to the planner. Each action is a
named Rust enum variant:

```rust
pub enum DaemonAction {
    InstallPackage    { name: String },
    AddFirewallRule   { rule: FirewallRule },
    PinDeployment     { hash: Option<String> },
    StartService      { name: String },
    // ... 80+ variants total
}
```

The planner (an LLM running locally via Ollama or via OpenAI / Anthropic /
Gemini) calls a `propose_plan` tool with a list of these actions. A safety
fence validates every action name and risk level before the plan leaves the
brain. The daemon — a separate privileged process — executes only what you
approve. Nothing else.

## What a session looks like

```
$ sysknife "pin the current deployment and block port 8080"

Plan (2 steps):
  1. PinDeployment     [Low]
     rpm-ostree pin 3a7f... (current deployment)

  2. AddFirewallRule   [Medium]
     firewall-cmd --add-rich-rule \
       'rule family="ipv4" port port="8080" protocol="tcp" drop'
     Side effects: rule persisted across reboots
     Rollback: RemoveFirewallRule (automatic on failure)

Approve step 1? [y/N] y
  ✓ PinDeployment succeeded. Audit entry #47.

Approve step 2? [y/N] y
  ✓ AddFirewallRule succeeded. Audit entry #48.

Chain: 3f9a...c12e
```

Two approvals, two audit entries. If step 2 had failed, the daemon would have
run `RemoveFirewallRule` automatically before surfacing the error.

## The architecture in one diagram

```
sysknife-brain   →   sysknife-shell   →   sysknife-daemon
  (planner)          (approval gate)        (executor)
  unprivileged       unprivileged           privileged
  talks to LLM       shows the plan,        polkit-guarded
  never to OS        takes y/n              signs every action
```

Three processes. Three separate trust domains. The brain is unprivileged and
network-connected (it talks to the LLM provider). The daemon is privileged and
network-isolated (it reads only local sockets and executes system commands).
The shell sits between them: it collects approval and passes signed requests to
the daemon. The brain never has a file descriptor that reaches the OS.

This boundary is mechanical. A compromised LLM response cannot bypass approval:
the shell enforces it. A shell bug cannot produce an undocumented OS action: the
daemon validates every request at the enum-match level before executing.

## The audit chain

Audit rows in SQLite are cheap but mutable. I wanted something you could
actually rely on after the fact — something where "I ran `sysknife audit verify`
and the chain is intact" means something.

Each row carries:

```
entry_hash = HMAC-SHA256(
  key     = machine_secret,
  message = canonical_json(action_result) || previous_entry_hash
)
```

The chain also carries an epoch watermark to detect clock rollbacks. Tamper
with any row — edit a result, delete a row, reorder two rows — and
`sysknife audit verify` fails immediately with the index of the first broken
link.

```
$ sysknife audit verify
Chain OK. 48 entries verified.
```

For homelab use this is a convenience. For regulated environments it is a
compliance primitive: RFC 5424 syslog forwarding to Splunk, Sentinel, or
QRadar ships out of the box, and the Postgres backend (with RDS / Cloud SQL /
Neon / Supabase support) is available for teams that need a central audit store.

## Why Rust specifically

Rust's enum exhaustiveness is what makes the typed-action model work in
practice. When I add a new action variant, the compiler tells me every match
arm in the daemon that needs to handle it. A missing case is a compile error,
not a runtime "unrecognised action" string that slips through to a shell.

The polkit integration also benefits from Rust's `?` propagation: every
privilege check either succeeds or returns an error that propagates cleanly to
the caller. No silent fallthrough.

The 1 026 tests running on every commit via `cargo nextest` give me confidence
that the type boundaries hold end-to-end. E2E stories drive the full
plan → approve → execute → audit cycle against a real daemon socket.

## What is not done yet

Silverblue and Fedora 41+ are fully supported. Ubuntu action families landed
in the codebase in the last sprint; the planner-side distro hints and live E2E
validation against an Ubuntu VM are still in progress. If you are on Ubuntu
today the CLI will plan correctly, but I would not call it validated yet.

Arch, NixOS, and other distros are roadmap items. The LACS spec (the typed
action protocol SysKnife implements) is CC0 — other implementations for other
distros are explicitly encouraged.

## Try it

```sh
# Fedora / Silverblue — full install
git clone https://github.com/lacs-foundation/sysknife
cd sysknife && make build && sudo make install
sudo systemctl enable --now sysknife-daemon
export OPENAI_API_KEY=sk-...   # or ANTHROPIC_API_KEY / GEMINI_API_KEY
sysknife --dry-run "show disk usage"

# No install needed — cloud-only dry run (plans only, no daemon)
export ANTHROPIC_API_KEY=sk-ant-...
sysknife --dry-run "show disk usage and list services that used cpu in the last hour"

# Fully offline with Ollama
ollama pull qwen3:8b
# set provider = "ollama" in ~/.config/sysknife/config.toml
sysknife "show disk usage"
```

The MCP server lets Claude Code and Cursor call `sysknife_plan` and
`sysknife_execute` directly. Setup with `npx sysknife-setup`.

---

Feedback welcome — especially bug reports from people running Silverblue as a
daily driver, and from anyone who tries the Ubuntu path.

- GitHub: <https://github.com/lacs-foundation/sysknife>
- Docs: <https://lacs-foundation.github.io/sysknife/>
- LACS spec: <https://github.com/lacs-foundation/specification>
