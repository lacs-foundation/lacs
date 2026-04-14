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

STEP_COUNT=$(echo "$PLAN" | jq '.steps | length')
if [[ "$STEP_COUNT" != "1" ]]; then
  echo "FAIL: expected 1 step, got $STEP_COUNT"
  echo "Actions: $(echo "$PLAN" | jq -r '.steps[].action_name')"
  exit 1
fi

ACTION=$(echo "$PLAN" | jq -r '.steps[0].action_name')
if [[ "$ACTION" != "AddUserToGroup" ]]; then
  echo "FAIL: expected AddUserToGroup, got $ACTION"
  exit 1
fi

USERNAME=$(echo "$PLAN" | jq -r '.steps[0].params.username // ""')
if [[ "$USERNAME" != "devops" ]]; then
  echo "FAIL: expected params.username=devops, got '$USERNAME'"
  echo "Full params: $(echo "$PLAN" | jq '.steps[0].params')"
  exit 1
fi

GROUP=$(echo "$PLAN" | jq -r '.steps[0].params.group // ""')
if [[ "$GROUP" != "wheel" ]]; then
  echo "FAIL: expected params.group=wheel, got '$GROUP'"
  echo "Full params: $(echo "$PLAN" | jq '.steps[0].params')"
  exit 1
fi

RISK=$(echo "$PLAN" | jq -r '.steps[0].risk_level')
if [[ "$RISK" != "high" ]]; then
  echo "FAIL: expected risk high, got $RISK"
  exit 1
fi

echo "PASS: Story 20 — plan has AddUserToGroup(username=devops, group=wheel) with high risk"
