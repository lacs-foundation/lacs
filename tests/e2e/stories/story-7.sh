#!/usr/bin/env bash
# Story 7: SSH key inventory
# Intent: "show me the SSH keys authorized for user lacsdev"
# Pass criteria:
#   - Plan has 1 step, GetAuthorizedKeys
#   - params.username == "lacsdev"
set -euo pipefail

INTENT="show me the SSH keys authorized for user lacsdev"

echo "=== Story 7: SSH key inventory ==="
echo "Intent: $INTENT"

PLAN=$(echo "$INTENT" | lacs-test-cli 2>/tmp/lacs-story-7-stderr.log)
echo "Plan JSON:"
echo "$PLAN" | jq .

# --- Assertions ---

STEP_COUNT=$(echo "$PLAN" | jq '.steps | length')
if [[ "$STEP_COUNT" != "1" ]]; then
  echo "FAIL: expected 1 step, got $STEP_COUNT"
  exit 1
fi

ACTION=$(echo "$PLAN" | jq -r '.steps[0].action_name')
if [[ "$ACTION" != "GetAuthorizedKeys" ]]; then
  echo "FAIL: expected GetAuthorizedKeys, got $ACTION"
  exit 1
fi

# Check username parameter.
USERNAME=$(echo "$PLAN" | jq -r '.steps[0].params.username // .steps[0].params.user // ""')
if [[ "$USERNAME" != "lacsdev" ]]; then
  echo "FAIL: expected username=lacsdev, got '$USERNAME'"
  echo "Full params: $(echo "$PLAN" | jq '.steps[0].params')"
  exit 1
fi

echo "PASS: Story 7 — plan has GetAuthorizedKeys for lacsdev"
