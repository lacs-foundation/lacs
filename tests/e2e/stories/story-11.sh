#!/usr/bin/env bash
# Story 11: Deployment status + kernel arguments (compound read-only)
# Intent: "show me the current deployment status and what kernel arguments are set"
# Pass criteria:
#   - Plan has exactly 2 steps
#   - Steps contain both GetKernelArguments and ListDeployments (any order)
#   - All steps have risk_level low
set -euo pipefail

INTENT="show me the current deployment status and what kernel arguments are set"

echo "=== Story 11: Deployment status + kernel arguments ==="
echo "Intent: $INTENT"

PLAN=$(echo "$INTENT" | lacs-test-cli 2>/tmp/lacs-story-11-stderr.log)
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

if ! echo "$ACTIONS" | grep -q "GetKernelArguments"; then
  echo "FAIL: GetKernelArguments not found in plan"
  echo "Actions: $ACTIONS"
  exit 1
fi

if ! echo "$ACTIONS" | grep -q "ListDeployments"; then
  echo "FAIL: ListDeployments not found in plan"
  echo "Actions: $ACTIONS"
  exit 1
fi

# All steps must be low risk (these are read-only).
RISKS=$(echo "$PLAN" | jq -r '.steps[].risk_level')
while IFS= read -r risk; do
  if [[ "$risk" != "low" ]]; then
    echo "FAIL: expected all steps low risk, got '$risk'"
    echo "Full risks: $RISKS"
    exit 1
  fi
done <<< "$RISKS"

echo "PASS: Story 11 — plan has GetKernelArguments + ListDeployments, all low risk"
