# Twitter/X Thread

> Thread: 10 tweets. Post immediately after the HN submission goes live.
> Link back to HN in tweet 2 to funnel votes.
> Replace [GIF URL] and [HN URL] before posting.

---

## Tweet 1 (hook)

I got tired of AI assistants pasting shell commands I had to run blind.

So I built SysKnife: describe a Linux task, the LLM proposes typed actions,
you approve, the daemon audits every execution.

The AI is not allowed to run shell. Here is how that works:

[GIF URL]

---

## Tweet 2 (HN link — post within first 5 min)

Just posted on Hacker News if you want to dig into the design:

[HN URL]

I will be answering questions there for the next few hours.

---

## Tweet 3 (the core constraint)

The key design choice:

The planner (LLM) can only propose actions from a fixed Rust enum. 80+
variants. Packages, services, firewall, containers, SSH keys, deployments.

If the model hallucinates an action name, the safety fence rejects the plan
before it reaches the daemon.

No shell string ever crosses the trust boundary.

---

## Tweet 4 (the three processes)

Three processes. Three trust domains.

```
brain  →  shell  →  daemon
(LLM)    (y/n)    (privileged)
```

The brain talks to the LLM. It never touches the OS.
The daemon touches the OS. It never talks to the LLM.
The shell sits between them and collects your approval.

The boundary is mechanical — not a soft "are you sure?" prompt.

---

## Tweet 5 (audit chain)

Every execution is appended to a HMAC-SHA256 hash chain.

Delete a row: chain breaks.
Modify a result: chain breaks.
Roll back the clock: epoch watermark catches it.

```
$ sysknife audit verify
Chain OK. 48 entries verified.
```

RFC 5424 syslog forwarding to Splunk / Sentinel / QRadar ships out of the box.

---

## Tweet 6 (local-first)

Works fully offline with Ollama. No API key. No data leaves your machine.

```toml
[llm]
provider   = "ollama"
model      = "qwen3:8b"
ollama_url = "http://localhost:11434"
```

qwen3:8b on a 16 GB laptop is the recommended homelab config.

---

## Tweet 7 (Silverblue detail — show specificity)

On Silverblue it does the right thing by default.

When I say "install neovim" on an immutable OS, the planner proposes
`rpm-ostree install neovim` — not `dnf install`. It knows the OS model.

Tested this specifically because a previous AI assistant suggested `dnf`
three times in a row on the same box.

---

## Tweet 8 (current scope — honest)

Current state, no overclaiming:

- Fedora 41+ / Silverblue 41+: fully supported, 80+ actions
- Ubuntu: action families implemented, planner integration + E2E still in
  progress — not yet validated for production use
- Arch / NixOS: roadmap
- 1 026 tests, AGPL-3.0 license

---

## Tweet 9 (the spec)

The protocol SysKnife implements — LACS (Linux Agent Control Standard) — is
CC0.

Typed actions + risk classification + approval gate + audit requirements,
specified so other distros and other languages can implement it.

github.com/lacs-foundation/specification

Other implementations explicitly encouraged.

---

## Tweet 10 (CTA)

Try it:

```sh
git clone https://github.com/lacs-foundation/sysknife
cd sysknife && make build && sudo make install
sudo systemctl enable --now sysknife-daemon
sysknife --dry-run "show disk usage"
```

Fully offline with Ollama. Full install guide:
<https://lacs-foundation.github.io/sysknife/quickstart.html>

Bugs, edge cases, distro requests: issues and Discussions welcome.
