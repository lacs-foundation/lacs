#!/usr/bin/env bash
# Exec story 7 (destructive): RestartService — service control mutation
# Intent: "restart firewalld"
# Pass criteria:
#   - sysknife executes successfully (exit 0)
#   - firewalld is still active after restart (restart is idempotent)
# Risk: Medium — uses printf 'y\n' | sysknife (no --yes: Medium needs explicit confirmation).
# firewalld is guaranteed on all Fedora Atomic desktops (provision.sh enables it).
set -euo pipefail

if [[ "${SYSKNIFE_ALLOW_DESTRUCTIVE:-0}" != "1" ]]; then
  echo "SKIP: set SYSKNIFE_ALLOW_DESTRUCTIVE=1 to run service mutation stories"
  exit 0
fi

INTENT="restart firewalld"

echo "=== Exec 7: RestartService(firewalld) ==="
echo "Intent: $INTENT"

OUTPUT=$(printf 'y\n' | sysknife "$INTENT" 2>/tmp/sysknife-exec-7-stderr.log)
echo "--- Output ---"
echo "$OUTPUT"

# Verify firewalld is still active after restart.
if systemctl is-active --quiet firewalld; then
  echo "PASS: Exec 7 — RestartService(firewalld) executed and service is still active"
else
  echo "FAIL: firewalld is not active after restart"
  echo "--- stderr ---"
  cat /tmp/sysknife-exec-7-stderr.log || true
  exit 1
fi
