# r/selfhosted — Thread Draft

## Title

**I built SysKnife: describe a Linux task in plain English, an LLM plans it
(Ollama works, fully offline), you approve, every action gets signed and
audited. MIT, Fedora-first.**

---

## Body

If you run Silverblue or Fedora at home and have ever wanted an AI assistant
that does not just spit out shell one-liners you have to run blind — this is
what I built.

![SysKnife demo](https://raw.githubusercontent.com/lacs-foundation/sysknife/main/assets/demo/demo.gif)

**The homelab pitch in one paragraph:**

SysKnife sits between your intent and your system. You say what you want in
plain language. The LLM (local via Ollama or cloud via OpenAI / Anthropic /
Gemini — your choice) produces a plan as a list of typed actions. You see each
step with its risk level, side effects, and rollback info before anything runs.
You approve. The daemon executes. Every action is written to a tamper-evident
hash-chained audit log you can verify at any time.

**Local-first / no API key required**

```toml
# ~/.config/sysknife/config.toml
[llm]
provider   = "ollama"
model      = "qwen3:8b"
ollama_url = "http://localhost:11434"
```

That is the full config for a fully offline setup. qwen3:8b runs fine on a
laptop with 16 GB RAM. No data leaves your machine.

**What it can do right now (Fedora 41+ / Silverblue 41+)**

- Layered package management (`rpm-ostree install / remove / pin`)
- Service management (`systemctl start / stop / enable / disable`)
- Firewall rules (`firewall-cmd`, persisted and audited)
- Container management (podman start / stop / pull / remove)
- Flatpak install / update / remove
- Toolbox create / enter / remove
- User and group management
- SSH key management
- Kernel argument management
- Deployment management (pin, rollback)
- Read-only queries: disk, memory, services, containers, packages — all
  usable without running anything

**The audit trail**

```
$ sysknife audit list
#45  2025-04-19 14:03  AddFirewallRule     Succeeded  hash: a3f2...
#46  2025-04-19 14:07  InstallPackage      Succeeded  hash: 7c1d...
#47  2025-04-19 15:11  PinDeployment       Succeeded  hash: 9e44...

$ sysknife audit verify
Chain OK. 47 entries verified.
```

This is the part I like for homelab: a year from now I can look back at the
log and know exactly what SysKnife did, when, and verify no entry was tampered
with.

**Current limits (being honest)**

- Ubuntu action families are in the codebase but planner integration and
  live E2E tests on Ubuntu are still in progress. If you are on Ubuntu
  today, the CLI plans correctly but execution is not yet validated.
- Arch / Pacman and NixOS are roadmap items, not current work.
- The GUI (Tauri shell) is functional but the CLI is the primary interface
  for now.

**Links**

- Repo: <https://github.com/lacs-foundation/sysknife>
- Quick start: <https://lacs-foundation.github.io/sysknife/quickstart.html>
- Distro matrix: <https://lacs-foundation.github.io/sysknife/distro-support.html>

Feedback welcome, especially from people running Silverblue — I have been
testing on it as a daily driver and I want to know what breaks in real usage.
