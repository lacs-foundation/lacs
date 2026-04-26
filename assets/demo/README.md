# Demo assets

`demo.tape` is a [VHS](https://github.com/charmbracelet/vhs) script that drives
the top-of-README demo GIF. The tape calls `demo-mock.sh`, a deterministic
offline replay that mirrors the styling of `apps/sysknife-cli/src/render.rs`.

We use a mock instead of the live `sysknife` CLI so the recording is
reproducible from a fresh checkout — no LLM key, daemon, or network
round-trip needed, and every regeneration produces the same frames.

## Regenerate

```bash
# Install VHS (first time only)
go install github.com/charmbracelet/vhs@latest
# or: brew install charmbracelet/tap/vhs

# Render
vhs assets/demo/demo.tape
```

Output: `demo.gif` — embedded in `README.md`.

## Sizing rules

- **Width × Height = 1200 × 720**, FontSize 24.
- **GIF should stay under 2 MB**; 3 MB hard ceiling. GitHub's mobile renderer
  chokes above that.

## Updating the recording

Edit `demo-mock.sh` to change the streamed lines, then re-run `vhs`. Keep the
mock visually faithful to the real CLI render (risk badges, ▶ step header,
› output line, ✓ success summary).

If you ever need to record against a real daemon (e.g. for a feature demo
that the mock cannot capture), point the .tape at the live CLI directly and
ensure `sysknife` is on `$PATH`, the daemon is running, and an LLM provider
key is set — but commit the resulting GIF only, not a tape that depends on
those side conditions.
