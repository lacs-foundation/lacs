#!/usr/bin/env bash
# Story 13: Service logs for a named unit (parameter extraction)
# Intent: "show me the logs for the firewalld service"
# Pass criteria:
#   - Plan has exactly 1 step: GetServiceLogs
#   - params.unit matches "firewalld" or "firewalld.service"
#   - risk_level low
#
# This story tests that the model correctly extracts a specific service name
# from the intent and maps it to the unit param without inventing extra steps.
set -euo pipefail

INTENT="show me the logs for the firewalld service"

echo "=== Story 13: Service logs for firewalld ==="
echo "Intent: $INTENT"

PLAN=$(echo "$INTENT" | lacs-test-cli 2>/tmp/lacs-story-13-stderr.log)
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
if [[ "$ACTION" != "GetServiceLogs" ]]; then
  echo "FAIL: expected GetServiceLogs, got $ACTION"
  exit 1
fi

RISK=$(echo "$PLAN" | jq -r '.steps[0].risk_level')
if [[ "$RISK" != "low" ]]; then
  echo "FAIL: expected risk low, got $RISK"
  exit 1
fi

# Accept "firewalld" or "firewalld.service" — both are valid unit names.
UNIT=$(echo "$PLAN" | jq -r '.steps[0].params.unit // ""')
if [[ "$UNIT" != "firewalld" && "$UNIT" != "firewalld.service" ]]; then
  echo "FAIL: expected unit=firewalld or firewalld.service, got '$UNIT'"
  echo "Full params: $(echo "$PLAN" | jq '.steps[0].params')"
  exit 1
fi

echo "PASS: Story 13 — plan has GetServiceLogs(unit=$UNIT) with low risk"
