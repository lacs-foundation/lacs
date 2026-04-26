# SysKnife MCP flow demo — VHS tape file (https://github.com/charmbracelet/vhs)
#
# Regenerate: `vhs assets/demo/mcp-flow.tape` (writes assets/demo/mcp-flow.gif)
#
# This tape calls assets/demo/mcp-flow-mock.sh, a deterministic offline replay
# of a Claude Code session using the sysknife_plan + sysknife_execute MCP tools.
# No live LLM calls, daemon, or network access required — every render is
# identical.

Output "assets/demo/mcp-flow.gif"

Set Theme         "Catppuccin Frappe"
Set FontSize      18
Set Width         1200
Set Height        720
Set Padding       20
Set BorderRadius  10
Set TypingSpeed   55ms

# Drive the recording through the mock script.
Sleep 400ms
Type "bash assets/demo/mcp-flow-mock.sh"
Enter

# Header bar + first user turn render (≈1.1s).
Sleep 1200ms

# Hold on the assistant acknowledgement + sysknife_plan spinner.
Sleep 1500ms

# Hold on the tool-result plan card so viewer can read risk pills.
Sleep 2500ms

# Assistant plan summary + approval prompt + "y" typed.
Sleep 2000ms

# sysknife_execute spinner + step 1 streaming (≈3s).
Sleep 4000ms

# Step 2 + step 3 streaming (≈3s).
Sleep 4000ms

# Hold on the final summary + audit hash.
Sleep 1500ms
