# Demo assets

`demo.tape` is a [VHS](https://github.com/charmbracelet/vhs) script that drives the
top-of-README demo GIF.

## Regenerate

```bash
# Install VHS (first time only)
go install github.com/charmbracelet/vhs@latest
# or: brew install charmbracelet/tap/vhs

# Render
vhs assets/demo/demo.tape
```

Outputs:

- `demo.gif` — embedded in `README.md`
- `demo.webm` — embedded in the launch landing page (smaller, autoplay-friendly)

## Sizing rules

- **Width × Height = 1200 × 700**, FontSize 36 — VHS's documented defaults.
- **GIF must stay under 2 MB**; under 5 MB hard-limit. GitHub's mobile renderer
  chokes above that.
- WebM is smaller than GIF and supported on every modern browser, but GitHub
  README rendering still prefers GIF — keep both.

## Re-recording the demo against a real daemon

The .tape commands assume `sysknife` is on `$PATH` and a running daemon is
reachable. Live regeneration requires:

1. `cargo install --path apps/sysknife-cli`
2. `systemctl --user start sysknife-daemon` (or run `cargo run -p sysknife-daemon` in a side terminal)
3. An LLM provider key in env (`OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / Ollama
   running locally).

If you don't have credentials, leave the GIF as-is — the bundled one was
recorded against a known-good plan.
