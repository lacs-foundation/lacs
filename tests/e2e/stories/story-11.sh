#!/usr/bin/env bash
# Story 11: Deployment status + kernel arguments (compound read-only)
# Intent: "show me the current deployment status and what kernel arguments are set"
# Pass criteria (any of these is acceptable):
#   A) Two steps: GetKernelArguments + ListDeployments/GetDeploymentHistory (in any order)
#   B) Single step: GetSystemState (covers both deployment state and kernel args)
#   - All steps have risk_level low
set -euo pipefail

INTENT="show me the current deployment status and what kernel arguments are set"

echo "=== Story 11: Deployment status + kernel arguments ==="
echo "Intent: $INTENT"

PLAN=$(echo "$INTENT" | lacs-test-cli 2>/tmp/lacs-story-11-stderr.log)
echo "Plan JSON:"
echo "$PLAN" | jq .

# --- Assertions ---

ACTIONS=$(echo "$PLAN" | jq -r '.steps[].action_name')

# Accept either the specific compound plan or the general-purpose fallback.
HAS_KERNEL=$(echo "$ACTIONS" | grep -c "GetKernelArguments" || true)
HAS_DEPLOY=$(echo "$ACTIONS" | grep -cE "ListDeployments|GetDeploymentHistory" || true)
HAS_STATE=$(echo "$ACTIONS" | grep -c "GetSystemState" || true)

if [[ "$HAS_STATE" -ge 1 ]]; then
  : # GetSystemState covers deployment status + kernel info — accepted
elif [[ "$HAS_KERNEL" -ge 1 && "$HAS_DEPLOY" -ge 1 ]]; then
  : # Specific compound plan — preferred
else
  echo "FAIL: expected (GetKernelArguments + deployment action) or GetSystemState"
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

echo "PASS: Story 11 — valid deployment+kernel plan, all low risk"
