# Show HN — Drafts

## Recommended title

**Show HN: SysKnife — describe a Linux task, the AI plans it, you approve, it audits**

### Rationale

This title names the tool, states the full loop in one breath (describe →
plan → approve → audit), and implies the differentiator without spelling it
out. "You approve" signals the human is in the loop. "Audits" signals trust
and compliance value. It is 11 words — short enough to scan, specific enough
to be falsifiable.

---

## Backup candidates

1. **Show HN: An AI sysadmin in Rust where the AI cannot run shell**
   — Provocative contrast with Open Interpreter and shell-gpt. The "cannot run
   shell" claim is technically precise and invites the follow-up "then how?"
   Risk: some readers may read it as a limitation rather than a feature.

2. **Show HN: SysKnife — plan-then-approve Linux administration with any LLM**
   — Neutral framing, emphasises provider flexibility (Ollama, OpenAI,
   Anthropic, etc.). Good if the Ollama / local-first angle is the lead.

3. **Show HN: I built a Linux agent in Rust with a typed action set and
   tamper-evident audit chain**
   — More technical, more specific. Likely to score well with the security /
   compliance crowd. Long — 18 words. Consider only if the "audit chain" angle
   is the hook of the week (e.g. after a high-profile breach).

4. **Show HN: SysKnife — your sysadmin co-pilot that logs every action it takes**
   — Friendly, accessible. "Co-pilot" is overused in 2024–2026 but still lands
   for non-specialists. "Logs every action" is concrete.

5. **Show HN: I gave an LLM a typed API to my Linux system instead of a shell**
   — First-person, framed as an experiment. Strong hook for HN readers who
   think about LLM tool design. Risk: implies research prototype rather than
   usable tool.

---

## Post body draft

```
SysKnife is a Linux sysadmin co-pilot that keeps the AI out of your shell.

[demo GIF — replace with actual URL once demo.gif is uploaded]
![SysKnife demo](https://raw.githubusercontent.com/lacs-foundation/sysknife/main/assets/demo/demo.gif)

Sample session:

    $ sysknife "pin the current deployment and block port 8080"

    Plan (2 steps):
      1. PinDeployment         [Low]    rpm-ostree pin <hash>
      2. AddFirewallRule        [Medium] firewall-cmd --add-rich-rule ...

    Approve step 1? [y/N] y
    Approve step 2? [y/N] y
    ✓ Both steps executed. Audit entry #47 written. Hash: 3f9a...c12e

The key constraint: the brain (an LLM running locally via Ollama or remotely
via OpenAI / Anthropic / Gemini) proposes a plan as a list of typed actions.
The daemon — a separate privileged process — executes only what you approve.
No shell string ever crosses the wire.

What this gets you:
- Every action is a typed Rust enum. No arbitrary command injection.
- Every execution is appended to a HMAC-SHA256 hash chain in SQLite.
  `sysknife audit verify` checks it.
- High-risk steps (rpm-ostree, user management) trigger automatic rollback
  on failure.
- RFC 5424 syslog forwarding to Splunk / Sentinel / QRadar ships out of the box.
- Works with Ollama (no API key, fully offline) or any cloud provider.

Current state: Fedora 41+ and Silverblue 41+ are fully supported.
Ubuntu action families landed; planner-side hints and live E2E validation
against a Ubuntu VM are in flight. MIT license. 1 026 tests pass on every
commit. 80+ typed actions.

The spec (LACS — Linux Agent Control Standard) is CC0 and lives separately at
github.com/lacs-foundation/specification. Other implementations are encouraged.

Repo: https://github.com/lacs-foundation/sysknife
Docs: https://lacs-foundation.github.io/sysknife/

I am happy to answer questions about the trust model, the daemon boundary,
the audit chain design, or why we chose typed actions over structured shell
templating.
```

---

## First comment template

Post this within five minutes of the thread going live.

```
Hi HN — I am Vladimir, I built SysKnife.

The one question I expect: "why not just prompt the LLM to produce a safe
shell command?" The short answer is that "safe shell" is not a type. A typed
action is. The daemon rejects anything that does not match a known action
variant at the Rust enum level — there is no string parsing in the hot path.

The audit chain was the other non-obvious piece. SQLite rows are cheap but
mutable; the HMAC-SHA256 chain means any tampering — including deleting a
row — breaks verification. `sysknife audit verify` walks the chain from the
genesis entry.

Happy to go deep on any of this: the polkit boundary, the IPC framing,
the provider adapter design, the rollback semantics, or the LACS spec itself.
I will be in the thread for the next few hours.

GitHub: https://github.com/lacs-foundation/sysknife
```
