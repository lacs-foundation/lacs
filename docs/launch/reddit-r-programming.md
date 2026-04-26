# r/programming — Thread Draft

## Title

**I gave an LLM a typed Rust API to my Linux system instead of a shell, with
a polkit-mediated daemon boundary and a HMAC-SHA256 audit chain. Here is what
I learned.**

---

## Body

The common pattern in AI sysadmin tools is: LLM produces a shell command,
tool executes it. Open Interpreter, shell-gpt, and dozens of smaller projects
follow this model. The surface is a string. The safety guarantee is "the LLM
probably wrote a reasonable command."

I wanted something different. SysKnife gives the LLM a typed API — a finite
set of Rust enums — and enforces that boundary at the OS privilege layer.

### The type system as a safety boundary

The brain (the LLM planner) can only propose actions from a fixed vocabulary:

```rust
pub enum DaemonAction {
    InstallPackage { name: String },
    RemovePackage  { name: String },
    AddFirewallRule { rule: FirewallRule },
    PinDeployment  { hash: Option<String> },
    // ... 80+ variants
}
```

When the planner calls `propose_plan`, the safety fence in `sysknife-brain`
validates every action name and risk level. If the LLM hallucinates an action
name, the plan is rejected before it reaches the daemon. No parsing, no
injection surface — just an enum match.

### The trust boundary

Three processes. No single process crosses its lane.

```
sysknife-brain   →   sysknife-shell   →   sysknife-daemon
  (planner)          (approval gate)        (executor)
  unprivileged       unprivileged           privileged, polkit-guarded
  talks to LLM       shows the plan,        only touches the OS
  never to OS        takes y/n              signs every action
```

The daemon is the only component that touches the OS. It communicates over a
Unix domain socket. The planner never has a file descriptor that reaches
privileged kernel interfaces. Privilege escalation in the brain cannot produce
an OS action — the daemon would need to execute it, and the daemon validates
every request independently.

### The audit chain

SQLite rows are cheap, but they are mutable. A naive audit log can be silently
edited. SysKnife appends a HMAC-SHA256 chain:

```
entry_hash = HMAC-SHA256(
  key     = machine_secret,
  message = canonical_json(action) || previous_entry_hash
)
```

`sysknife audit verify` walks the chain from the genesis entry. Any deleted,
reordered, or modified row breaks the chain immediately. The chain also carries
an epoch watermark so clock rollbacks are detectable.

### The rollback semantics

High-risk actions carry rollback metadata in their preview:

```rust
pub struct ActionPreview {
    pub risk:            RiskLevel,
    pub side_effects:    Vec<String>,
    pub rollback_action: Option<DaemonAction>,
    pub reboot_required: bool,
    pub content_hash:    String,
}
```

If a High-risk step fails, the daemon executes the rollback action
automatically before surfacing the error. For `rpm-ostree` this means
`rpm-ostree rollback`. For firewall rules it means removing the newly added
rule.

### What I got wrong the first time

The original design had the approval gate in the brain. The brain would plan,
then ask for approval, then execute. This breaks the trust boundary: the brain
is unprivileged and talks to the LLM — a remote network service. Approval must
live in the shell (or daemon), not in the planning layer. A compromised LLM
response cannot bypass approval by claiming it already got it.

### Current state and limitations

- Fedora 41+ and Silverblue 41+ are fully supported with all 80+ actions.
- Ubuntu action families are implemented; the planner hints and live E2E
  validation against a Ubuntu VM are still in progress.
- The protocol (LACS — Linux Agent Control Standard) is CC0 at
  <https://github.com/lacs-foundation/specification>.
- 1 026 tests. AGPL-3.0 license.
- Repo: <https://github.com/lacs-foundation/sysknife>

I am happy to discuss the type design, the IPC framing, the provider adapter
pattern, or any of the security properties — in the comments or in GitHub
Discussions.
