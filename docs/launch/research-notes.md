# Launch Research Notes

## Sources consulted

- [Show HN Guidelines](https://news.ycombinator.com/showhn.html)
- [How to crush your Hacker News launch — DEV Community](https://dev.to/dfarrell/how-to-crush-your-hacker-news-launch-10jk)
- [How to absolutely crush your Hacker News launch — Onlook](https://onlook.substack.com/p/launching-on-hacker-news)
- [How to launch a dev tool on Hacker News — markepear.dev](https://www.markepear.dev/blog/dev-tool-hacker-news-launch)
- [How to Launch on Hacker News: A Practical Guide to Getting 500+ Upvotes — Calmops](https://calmops.com/indie-hackers/hacker-news-launch-500-upvotes/)
- [Lessons launching a developer tool on HN vs Product Hunt — Medium](https://medium.com/@baristaGeek/lessons-launching-a-developer-tool-on-hacker-news-vs-product-hunt-and-other-channels-27be8784338b)
- [Best time to post on Show HN — Myriade](https://www.myriade.ai/blogs/when-is-it-the-best-time-to-post-on-show-hn)
- [The best time to post on Hacker News — alcazarsec](https://blog.alcazarsec.com/tech/posts/best-time-to-post-on-hacker-news)
- [Show HN: Open Interpreter — CodeLlama in your terminal, executing code](https://news.ycombinator.com/item?id=37315866)
- [Aider: AI pair programming in your terminal — HN](https://news.ycombinator.com/item?id=39995725)
- [Show HN: Pica — Rust-based agentic AI infrastructure](https://news.ycombinator.com/item?id=42781017)
- [Show HN: OpenClaw Harness — Security firewall for AI coding agents (Rust)](https://news.ycombinator.com/item?id=46854108)
- [Self-Hosted Survey 2024](https://selfhosted-survey-2024.deployn.de/)
- [My Favorite Self-Hosted Apps Launched in 2025 — selfh.st](https://selfh.st/post/2025-favorite-new-apps/)
- [Why Self-Hosting Really Works in 2025 — DreamHost](https://www.dreamhost.com/blog/self-hosting/)
- [Why Developers Are Switching to Rust — DEV Community](https://dev.to/shah_bhoomi_fc7f7c4305283/why-developers-are-switching-to-rust-the-rise-of-rust-development-in-2025-3p5l)
- [10 Must-have CLIs for your AI Agents in 2026 — Medium](https://medium.com/@unicodeveloper/10-must-have-clis-for-your-ai-agents-in-2026-51ba0d0881df)

---

## Patterns extracted

### P1 — Show HN titles follow `Tool — one-line differentiator` or `I built X that does Y`

Winning titles name the tool, then state the unusual thing it does — not the
category. "Open Interpreter — CodeLlama in your terminal, executing code" beats
"An AI terminal assistant." The unusual claim should be falsifiable and
specific: "executing code" signals real capability. "where the AI can't run
shell" is likewise concrete.

### P2 — Post body opens with a concrete artefact, not mission statement

High-scoring Show HN bodies lead with a GIF, a four-line code block, or a
sample output. Mission statements ("I built this because…") work as paragraph
two. The demo earns attention; the story earns trust.

### P3 — First comment is the builder announcing presence

The canonical pattern across every well-cited HN launch guide: post goes live,
then within minutes the builder drops a first comment along the lines of
"Hi HN — I'm [name], I built this. Happy to answer anything here." This seeds
the thread and signals the author is responsive. Open Interpreter, Aider, and
dozens of smaller launches all did this.

### P4 — Tuesday–Thursday, 8–10 AM Pacific is the consensus sweet spot

Analysis of 23 000+ posts shows the first-half-of-week morning window
consistently produces the highest median scores. Weekend midnight Pacific
(lower competition) is a contrarian option but less predictable. Avoid Friday
afternoon.

### P5 — r/linux and r/selfhosted reward direct demo over feature lists

Threads that hit 500+ upvotes in both subs share a pattern: title starts with
"I built" or "I made", body has a GIF or screenshot in the first fold,
feature list is short (5–7 bullets), and the author engages every top-level
comment. r/selfhosted in particular rewards Ollama / local-only setups — the
2024 community survey shows 97% of respondents use containers; the homelab
audience is privacy-first and wants offline-capable tools.

### P6 — dev.to Rust articles with traction use a problem-solution structure

The top-reacted Rust articles on dev.to (2024–2025) follow: hook (one
surprising fact or hard-won lesson) → problem section → how Rust's ownership
model forces a cleaner design → code block showing the before/after → takeaway.
They avoid language-war framing and assume the reader knows C or Python.

### P7 — Medium articles that drove signups are story-driven, not tutorial-driven

The Medium pieces that generated sign-up traffic read like engineering
post-mortems: "I tried X, it broke on Y, I reached for Z." They end with a
concrete call to action (GitHub link, newsletter). They are 2 000–3 000 words.
They use the first person and cite one or two negative facts ("this does not
work on Arch yet") — honesty converts readers better than hype.

### P8 — Cross-posting cadence matters; don't fire all channels at once

The standard playbook from multiple launch guides: HN first (peak audience,
signals legitimacy), then Reddit 4–12 h later (different audience, links to the
now-live HN thread as social proof), then long-form articles 24–48 h after
(content lives longer, drives organic search). Twitter/X thread can go same-day
as HN, but should link back to HN to funnel upvotes.
