# SysKnife demo — VHS tape file (https://github.com/charmbracelet/vhs)
#
# Regenerate: `vhs assets/demo/demo.tape` (writes assets/demo/demo.gif)
# Sweet spot from research: 1200x700, FontSize 36, ~30 seconds, ≤2 MB.
# Theme picked for visual continuity with the brand mint + signal-orange pills.

Output assets/demo/demo.gif
Output assets/demo/demo.webm

Set Theme         "Catppuccin Frappe"
Set FontSize      36
Set Width         1200
Set Height        700
Set Padding       20
Set BorderRadius  10
Set TypingSpeed   80ms

# ── Cold prompt ────────────────────────────────────────────────────────
Sleep 800ms
Type 'sysknife "install vim, restart sshd, and show me the firewall state"'
Sleep 500ms
Enter

# ── Planning spinner ──────────────────────────────────────────────────
Sleep 2s

# ── Plan card renders. Hold so the reader can read the risk pills ────
Sleep 5s

# ── Approve ────────────────────────────────────────────────────────────
Type "y"
Sleep 200ms
Enter

# ── Streaming execution ───────────────────────────────────────────────
Sleep 8s

# ── Final summary ─────────────────────────────────────────────────────
Sleep 4s

# ── Hold the final frame on the audit-log message ────────────────────
Sleep 2s
