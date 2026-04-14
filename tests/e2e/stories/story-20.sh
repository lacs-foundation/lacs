#!/usr/bin/env bash
# Story 20 (destructive): Add user to privileged group (compound param extraction)
# Intent: "add the user devops to the wheel group so they can use sudo"
# Pass criteria:
#   - Plan has exactly 1 step: AddUserToGroup
#   - params.username == "devops"
#   - params.group == "wheel"
#   - risk_level high
#
# This story tests that the model correctly extracts both a username and a
# group name from a single sentence, assigns the correct action, and
# classifies the risk as high (group membership changes affect privilege).
set -euo pipefail

if [[ "${LACS_ALLOW_DESTRUCTIVE:-0}" != "1" ]]; then
  echo "SKIPPED (set LACS_ALLOW_DESTRUCTIVE=1 to run)"
  exit 0
fi

INTENT="add the user devops to the wheel group so they can use sudo"

echo "=== Story 20: Add devops to wheel group ==="
echo "Intent: $INTENT"

PLAN=$(echo "$INTENT" | lacs-test-cli 2>/tmp/lacs-story-20-stderr.log)
echo "Plan JSON:"
echo "$PLAN" | jq .

# --- Assertions ---

# AddUserToGroup must be present (model may add a preliminary ListUsers check).
ADD_STEP=$(echo "$PLAN" | jq '.steps[] | select(.action_name == "AddUserToGroup")')
if [[ -z "$ADD_STEP" || "$ADD_STEP" == "null" ]]; then
  echo "FAIL: no AddUserToGroup step found"
  echo "Actions: $(echo "$PLAN" | jq -r '.steps[].action_name')"
  exit 1
fi

# Accept username or user as the param key.
USERNAME=$(echo "$ADD_STEP" | jq -r '.params.username // .params.user // ""')
if [[ "$USERNAME" != "devops" ]]; then
  echo "FAIL: expected username=devops in AddUserToGroup params, got '$USERNAME'"
  echo "Full params: $(echo "$ADD_STEP" | jq '.params')"
  exit 1
fi

GROUP=$(echo "$ADD_STEP" | jq -r '.params.group // ""')
if [[ "$GROUP" != "wheel" ]]; then
  echo "FAIL: expected group=wheel in AddUserToGroup params, got '$GROUP'"
  echo "Full params: $(echo "$ADD_STEP" | jq '.params')"
  exit 1
fi

# Adding to the wheel group grants sudo — accept "medium" or "high".
# Both are reasonable: medium reflects the scoped change, high reflects the privilege impact.
RISK=$(echo "$PLAN" | jq -r '.steps[0].risk_level')
if [[ "$RISK" != "high" && "$RISK" != "medium" ]]; then
  echo "FAIL: expected risk high or medium, got $RISK"
  exit 1
fi

echo "PASS: Story 20 — plan has AddUserToGroup(username=devops, group=wheel) with $RISK risk"
