# SysKnife Launch Playbook

> Phase 4 launch guide. Copy-paste cadence, not a wish list.

## Pre-launch checklist (T-7 days)

- [ ] Demo GIF recorded on real Silverblue hardware and committed to `assets/demo/`
- [ ] README hero image renders correctly on GitHub (check both light and dark mode)
- [ ] Repository topics set: `linux`, `rust`, `ai`, `sysadmin`, `audit`, `fedora`,
      `llm`, `cli`, `self-hosted`
- [ ] Branch protection on `main`: require PR + CI green before merge
- [ ] Pinned issue: roadmap / "what's next" so first-time visitors have context
- [ ] `npx sysknife-setup` tested end-to-end on a clean Fedora 41 VM
- [ ] `sysknife --dry-run "show disk usage"` tested with at least two LLM providers
- [ ] `sysknife audit verify` tested on the SQLite backend
- [ ] All five Show HN title candidates reviewed; one selected (see `show-hn.md`)
- [ ] `docs/launch/` drafts proof-read and copy-edited
- [ ] GitHub Discussions enabled; welcome thread pinned
- [ ] `CONTRIBUTING.md` accurate — at minimum the "good first issue" label exists
      with at least one open issue attached to it

---

## T-0 — Show HN post

**When:** Tuesday morning, 08:30–09:30 Pacific Time.

Rationale: the 8–10 AM PT Tuesday–Thursday window consistently produces the
highest median scores across large-sample HN analyses (see `research-notes.md`,
P4). Tuesday gives the post the full workweek to accumulate votes and get picked
up by TLDR, Hacker Newsletter, and similar aggregators by Thursday.

**Steps:**

1. Post the Show HN using the title from `show-hn.md` (recommended candidate).
2. Within five minutes, post the first comment from `show-hn.md`
   ("Hi HN — I'm Vladimir…"). This seeds the thread and signals you are watching.
3. Pin a link to the HN thread in the GitHub repository's social preview or
   pinned Discussion.
4. Post the Twitter/X thread from `twitter-thread.md` immediately after,
   linking back to the HN submission.

---

## T+0 to T+4h — Engage the HN thread

- Reply to every top-level comment within the first four hours.
- Prioritise technical questions about the trust model, the daemon boundary,
  and the audit chain — these are the questions HN readers care about most.
- If someone asks about a distro not yet supported: acknowledge it honestly,
  link to `docs/distro-support.md`, and point them to the relevant tracking issue.
- Do not delete or edit the original post. Do not ask for upvotes.

---

## T+4h — r/linux post (or next morning if HN thread is still active)

If the HN thread is still on the front page at T+4h, wait until the next
morning (Wednesday) to post on r/linux — you want Reddit to function as a
second wave, not split attention. If HN has cooled, post r/linux at T+4h.

Use the draft in `reddit-r-linux.md`. Observe r/linux rules:

- No direct ask for upvotes in the body.
- Do not cross-post simultaneously to multiple subs; stagger by at least
  six hours.
- Flair as `Tool` or `Project`.

---

## T+12h — r/selfhosted post

Use the draft in `reddit-r-selfhosted.md`. The homelab / self-hosted angle
emphasises Ollama support (no API key, works offline) and the audit chain
(something to show the family when they ask why you run your own server).

---

## T+24h — dev.to article

Publish the article from `devto-article.md`. Tag with `rust`, `linux`,
`opensource`, `sysadmin`. The dev.to article lives longer than the HN post
and drives organic search traffic for queries like "AI sysadmin rust" and
"Linux agent with audit trail."

---

## T+48h — Medium long-form

Publish the article from `medium-article.md`. The Medium piece targets a
broader audience — engineers who follow "I built" stories but are not
necessarily Linux sysadmins. Republishing from dev.to is fine (canonical URL
pointing to dev.to avoids duplicate-content concerns).

---

## T+3 days — r/programming post

Use the draft in `reddit-r-programming.md`. This is the Rust + AI agent +
trust model angle — more CS-heavy than the r/linux or r/selfhosted posts.
r/programming readers respond to technical design decisions, not feature lists.

---

## T+1 week — retrospective post

Write a short post on the project's GitHub Discussions (or a second HN
comment if the thread is still alive) with:

- Actual metrics: GitHub stars, installs, issues opened, PRs opened by
  contributors
- Surprises: what questions came up that you did not expect
- What you are doing next

This closes the loop for people who starred but did not engage at launch,
and gives journalists / newsletter authors a follow-up hook.

---

## Channel summary

| Channel | Timing | Draft |
|---|---|---|
| Hacker News (Show HN) | T+0, Tue 08:30–09:30 PT | `show-hn.md` |
| Twitter/X thread | T+0 immediately after HN | `twitter-thread.md` |
| r/linux | T+4h (or Wed morning) | `reddit-r-linux.md` |
| r/selfhosted | T+12h | `reddit-r-selfhosted.md` |
| dev.to | T+24h | `devto-article.md` |
| Medium | T+48h | `medium-article.md` |
| r/programming | T+3 days | `reddit-r-programming.md` |
| HN / Discussions retrospective | T+1 week | (write fresh) |
