#!/usr/bin/env bash
# Story 16: Network status + firewall (compound read-only)
# Intent: "show me the network status and the current firewall rules"
# Pass criteria:
#   - Plan has exactly 2 steps
#   - Steps contain both GetNetworkStatus and GetFirewallState (any order)
#   - All steps have risk_level low
set -euo pipefail

INTENT="show me the network status and the current firewall rules"

echo "=== Story 16: Network status + firewall rules ==="
echo "Intent: $INTENT"

PLAN=$(echo "$INTENT" | lacs-test-cli 2>/tmp/lacs-story-16-stderr.log)
echo "Plan JSON:"
echo "$PLAN" | jq .

# --- Assertions ---

STEP_COUNT=$(echo "$PLAN" | jq '.steps | length')
if [[ "$STEP_COUNT" != "2" ]]; then
  echo "FAIL: expected 2 steps, got $STEP_COUNT"
  echo "Actions: $(echo "$PLAN" | jq -r '.steps[].action_name')"
  exit 1
fi

ACTIONS=$(echo "$PLAN" | jq -r '.steps[].action_name')

if ! echo "$ACTIONS" | grep -q "GetNetworkStatus"; then
  echo "FAIL: GetNetworkStatus not found in plan"
  echo "Actions: $ACTIONS"
  exit 1
fi

if ! echo "$ACTIONS" | grep -q "GetFirewallState"; then
  echo "FAIL: GetFirewallState not found in plan"
  echo "Actions: $ACTIONS"
  exit 1
fi

RISKS=$(echo "$PLAN" | jq -r '.steps[].risk_level')
while IFS= read -r risk; do
  if [[ "$risk" != "low" ]]; then
    echo "FAIL: expected all steps low risk, got '$risk'"
    exit 1
  fi
done <<< "$RISKS"

echo "PASS: Story 16 — plan has GetNetworkStatus + GetFirewallState, all low risk"
