#!/usr/bin/env bash
# Story 19 (destructive): Full system update
# Intent: "update my Fedora Silverblue system"
# Pass criteria:
#   - Plan has exactly 1 step: UpdateSystem
#   - risk_level high
#   - approvalRequired true (implicit from high risk)
#
# UpdateSystem is the most consequential single action in LACS — it stages
# a new OSTree commit and requires a reboot. Risk must be classified as high.
set -euo pipefail

if [[ "${LACS_ALLOW_DESTRUCTIVE:-0}" != "1" ]]; then
  echo "SKIPPED (set LACS_ALLOW_DESTRUCTIVE=1 to run)"
  exit 0
fi

INTENT="update my Fedora Silverblue system"

echo "=== Story 19: Full system update ==="
echo "Intent: $INTENT"

PLAN=$(echo "$INTENT" | lacs-test-cli 2>/tmp/lacs-story-19-stderr.log)
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
if [[ "$ACTION" != "UpdateSystem" ]]; then
  echo "FAIL: expected UpdateSystem, got $ACTION"
  exit 1
fi

RISK=$(echo "$PLAN" | jq -r '.steps[0].risk_level')
if [[ "$RISK" != "high" ]]; then
  echo "FAIL: expected risk high, got $RISK"
  exit 1
fi

echo "PASS: Story 19 — plan has UpdateSystem with high risk"
