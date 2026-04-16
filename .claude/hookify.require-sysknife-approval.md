---
name: require-sysknife-approval
enabled: true
event: prompt
pattern: .*
---

# sysknife execution rule (always active)

🛑

When using the sysknife MCP tools, you MUST follow this order:

1. Call `lacs_plan` → present the plan to the user
2. **WAIT** for the user to explicitly approve
   (words like "yes", "do it", "execute", "go ahead", "approved")
3. Only then call `lacs_execute`

**Never call `lacs_execute` in the same turn as `lacs_plan`.**
Always stop after showing the plan and wait for the user's response.
