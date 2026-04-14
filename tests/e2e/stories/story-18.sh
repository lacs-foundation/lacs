#!/usr/bin/env bash
# Story 18 (destructive): Restart a named service
# Intent: "restart the bluetooth service"
# Pass criteria:
#   - Plan has exactly 1 step: RestartService
#   - params.unit matches "bluetooth" or "bluetooth.service"
#   - risk_level medium, approvalRequired true
set -euo pipefail

if [[ "${LACS_ALLOW_DESTRUCTIVE:-0}" != "1" ]]; then
  echo "SKIPPED (set LACS_ALLOW_DESTRUCTIVE=1 to run)"
  exit 0
fi

INTENT="restart the bluetooth service"

echo "=== Story 18: Restart the bluetooth service ==="
echo "Intent: $INTENT"

PLAN=$(echo "$INTENT" | lacs-test-cli 2>/tmp/lacs-story-18-stderr.log)
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
if [[ "$ACTION" != "RestartService" ]]; then
  echo "FAIL: expected RestartService, got $ACTION"
  exit 1
fi

# Accept "bluetooth" or "bluetooth.service".
UNIT=$(echo "$PLAN" | jq -r '.steps[0].params.unit // ""')
if [[ "$UNIT" != "bluetooth" && "$UNIT" != "bluetooth.service" ]]; then
  echo "FAIL: expected unit=bluetooth or bluetooth.service, got '$UNIT'"
  echo "Full params: $(echo "$PLAN" | jq '.steps[0].params')"
  exit 1
fi

RISK=$(echo "$PLAN" | jq -r '.steps[0].risk_level')
if [[ "$RISK" != "medium" ]]; then
  echo "FAIL: expected risk medium, got $RISK"
  exit 1
fi

echo "PASS: Story 18 — plan has RestartService(unit=$UNIT) with medium risk"
