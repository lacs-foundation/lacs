#!/usr/bin/env bash
# Story 6: Running containers overview
# Intent: "list all running containers and show me which services are up"
# Pass criteria:
#   - Plan has 2 steps, both risk low
#   - ListContainers and ListServices both present
set -euo pipefail

INTENT="list all running containers and show me which services are up"

echo "=== Story 6: Running containers overview ==="
echo "Intent: $INTENT"

PLAN=$(echo "$INTENT" | lacs-test-cli 2>/tmp/lacs-story-6-stderr.log)
echo "Plan JSON:"
echo "$PLAN" | jq .

# --- Assertions ---

STEP_COUNT=$(echo "$PLAN" | jq '.steps | length')
if [[ "$STEP_COUNT" != "2" ]]; then
  echo "FAIL: expected 2 steps, got $STEP_COUNT"
  exit 1
fi

# Both steps low risk.
for i in 0 1; do
  RISK=$(echo "$PLAN" | jq -r ".steps[$i].risk_level")
  if [[ "$RISK" != "low" ]]; then
    echo "FAIL: step $i risk is $RISK, expected low"
    exit 1
  fi
done

# Contains ListContainers and ListServices (in either order).
ACTIONS=$(echo "$PLAN" | jq -r '.steps[].action_name' | sort)
EXPECTED=$'ListContainers\nListServices'
if [[ "$ACTIONS" != "$EXPECTED" ]]; then
  echo "FAIL: expected ListContainers + ListServices, got:"
  echo "$ACTIONS"
  exit 1
fi

echo "PASS: Story 6 — plan has ListContainers + ListServices, both low risk"
