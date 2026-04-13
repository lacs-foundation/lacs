#!/usr/bin/env bash
# Story 4: Firewall inspection
# Intent: "what ports are currently open on the firewall?"
# Pass criteria:
#   - Plan has 1 step, GetFirewallState
set -euo pipefail

INTENT="what ports are currently open on the firewall?"

echo "=== Story 4: Firewall inspection ==="
echo "Intent: $INTENT"

PLAN=$(echo "$INTENT" | lacs-test-cli 2>/tmp/lacs-story-4-stderr.log)
echo "Plan JSON:"
echo "$PLAN" | jq .

# --- Assertions ---

# 1. At least 1 step present.
STEP_COUNT=$(echo "$PLAN" | jq '.steps | length')
if [[ "$STEP_COUNT" == "0" ]]; then
  echo "FAIL: plan has no steps"
  exit 1
fi

# 2. Contains GetFirewallState.
HAS_FW=$(echo "$PLAN" | jq '[.steps[] | select(.action_name == "GetFirewallState")] | length')
if [[ "$HAS_FW" == "0" ]]; then
  echo "FAIL: no GetFirewallState step found"
  echo "Actions present: $(echo "$PLAN" | jq -r '.steps[].action_name')"
  exit 1
fi

echo "PASS: Story 4 — plan includes GetFirewallState"
