#!/usr/bin/env bash
# LACS E2E test harness — runs user stories against a provisioned VM.
#
# Usage:
#   sudo tests/e2e/run-stories.sh          # run read-only stories 1-7
#   sudo LACS_ALLOW_DESTRUCTIVE=1 tests/e2e/run-stories.sh   # all 10
#   sudo tests/e2e/run-stories.sh 3 5 7    # run specific stories
#
# Prerequisites:
#   - /var/lib/lacs-e2e/ready exists (provisioning complete)
#   - lacs-daemon systemd service is running
#   - lacs-test-cli is installed in PATH
#   - Ollama is running with a model pulled
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="$SCRIPT_DIR/logs"
STORY_DIR="$SCRIPT_DIR/stories"

mkdir -p "$LOG_DIR"

# ---------------------------------------------------------------------------
# Preflight checks
# ---------------------------------------------------------------------------

preflight_ok=true

if [[ ! -f /var/lib/lacs-e2e/ready ]]; then
  echo "ERROR: /var/lib/lacs-e2e/ready not found. Run provisioning first."
  preflight_ok=false
fi

if ! systemctl is-active --quiet lacs-daemon 2>/dev/null; then
  echo "ERROR: lacs-daemon is not running."
  preflight_ok=false
fi

if ! command -v lacs-test-cli &>/dev/null; then
  echo "ERROR: lacs-test-cli not found in PATH."
  preflight_ok=false
fi

if ! command -v jq &>/dev/null; then
  echo "ERROR: jq not found in PATH."
  preflight_ok=false
fi

if [[ "$preflight_ok" != "true" ]]; then
  echo ""
  echo "Preflight checks failed. Aborting."
  exit 1
fi

# ---------------------------------------------------------------------------
# LLM + daemon socket env for lacs-test-cli
# ---------------------------------------------------------------------------
# The test CLI's BrainConfig::from_env() defaults to Anthropic, and the
# DaemonIpcClient defaults to /tmp/lacs-daemon.sock — neither matches our
# provisioned VM. Force the right values here so individual story scripts
# don't need to know or care.
export LACS_LLM_PROVIDER="${LACS_LLM_PROVIDER:-ollama}"
export LACS_LLM_MODEL="${LACS_LLM_MODEL:-${LACS_TEST_MODEL:-qwen3:8b}}"
export LACS_OLLAMA_URL="${LACS_OLLAMA_URL:-http://127.0.0.1:11434}"
# lacs-daemon's packaged systemd unit binds /run/lacs/daemon.sock. The
# test CLI defaults to /tmp/lacs-daemon.sock — force the real path via
# LACS_LISTEN_URI (the var the brain and shell clients both honour).
export LACS_LISTEN_URI="${LACS_LISTEN_URI:-unix:///run/lacs/daemon.sock}"

# ---------------------------------------------------------------------------
# Determine which stories to run
# ---------------------------------------------------------------------------

ALLOW_DESTRUCTIVE="${LACS_ALLOW_DESTRUCTIVE:-0}"

# Timeout per story (seconds). With qwen3:8b on host GPU, stories
# finish in <60 s; with llama3.2:3b on 4 vCPU CPU, 2–4 min; with
# qwen3:8b on CPU, impractical. 600 s is generous for the GPU path
# and tolerant of the CPU fallback. Override with LACS_STORY_TIMEOUT.
STORY_TIMEOUT="${LACS_STORY_TIMEOUT:-600}"

declare -A STORY_NAMES
STORY_NAMES[1]="Check disk usage"
STORY_NAMES[2]="Memory pressure diagnosis"
STORY_NAMES[3]="Service health check"
STORY_NAMES[4]="Firewall inspection"
STORY_NAMES[5]="List layered packages"
STORY_NAMES[6]="Running containers overview"
STORY_NAMES[7]="SSH key inventory"
STORY_NAMES[8]="Layer vim via rpm-ostree (destructive)"
STORY_NAMES[9]="Create a toolbox (destructive)"
STORY_NAMES[10]="Add SSH authorized key (destructive)"
STORY_NAMES[11]="Deployment status + kernel arguments"
STORY_NAMES[12]="LACS activity log — today"
STORY_NAMES[13]="Service logs for firewalld"
STORY_NAMES[14]="Triple compound — disk + memory + services"
STORY_NAMES[15]="Rollback history"
STORY_NAMES[16]="Network status + firewall rules"
STORY_NAMES[17]="Container list + specific info"
STORY_NAMES[18]="Restart bluetooth service (destructive)"
STORY_NAMES[19]="Update system (destructive)"
STORY_NAMES[20]="Add user to wheel group (destructive)"

declare -A RESULTS
declare -A DURATIONS
declare -A MESSAGES

if [[ $# -gt 0 ]]; then
  # Run specific stories passed as arguments.
  STORIES=("$@")
else
  if [[ "$ALLOW_DESTRUCTIVE" == "1" ]]; then
    STORIES=(1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20)
  else
    STORIES=(1 2 3 4 5 6 7 11 12 13 14 15 16 17)
  fi
fi

# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------

run_story() {
  local n="$1"
  local script="$STORY_DIR/story-${n}.sh"
  local log="$LOG_DIR/story-${n}.log"
  local name="${STORY_NAMES[$n]:-Story $n}"

  if [[ ! -f "$script" ]]; then
    RESULTS[$n]="SKIP"
    MESSAGES[$n]="script not found: $script"
    DURATIONS[$n]="0.0"
    return
  fi

  # Destructive stories (8-10, 18-20) require explicit opt-in.
  if { [[ "$n" -ge 8 && "$n" -le 10 ]] || [[ "$n" -ge 18 && "$n" -le 20 ]]; } \
      && [[ "$ALLOW_DESTRUCTIVE" != "1" ]]; then
    RESULTS[$n]="SKIP"
    MESSAGES[$n]="set LACS_ALLOW_DESTRUCTIVE=1 to run"
    DURATIONS[$n]="0.0"
    return
  fi

  echo -n "  Story $n ($name): "

  local start_time
  start_time=$(date +%s.%N)

  if timeout "$STORY_TIMEOUT" bash "$script" > "$log" 2>&1; then
    RESULTS[$n]="PASS"
    MESSAGES[$n]=""
  else
    local exit_code=$?
    RESULTS[$n]="FAIL"
    # Extract the last non-empty line as the failure message.
    MESSAGES[$n]=$(tail -n 5 "$log" | grep -v '^$' | tail -n 1)
    if [[ $exit_code -eq 124 ]]; then
      MESSAGES[$n]="timed out after ${STORY_TIMEOUT}s"
    fi
  fi

  local end_time
  end_time=$(date +%s.%N)
  DURATIONS[$n]=$(echo "$end_time - $start_time" | bc 2>/dev/null || echo "?")

  echo "${RESULTS[$n]} (${DURATIONS[$n]}s)"
}

# ---------------------------------------------------------------------------
# Execute
# ---------------------------------------------------------------------------

echo ""
echo "LACS E2E Test Run"
echo "================="
echo "Date:        $(date --iso-8601=seconds)"
echo "Stories:     ${STORIES[*]}"
echo "Destructive: $ALLOW_DESTRUCTIVE"
echo "Timeout:     ${STORY_TIMEOUT}s per story"
echo ""

for n in "${STORIES[@]}"; do
  run_story "$n"
done

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

echo ""
echo "================================================================"
echo "  RESULTS"
echo "================================================================"

pass_count=0
fail_count=0
skip_count=0

for n in "${STORIES[@]}"; do
  local_name="${STORY_NAMES[$n]:-Story $n}"
  local_result="${RESULTS[$n]}"
  local_duration="${DURATIONS[$n]}"
  local_msg="${MESSAGES[$n]}"

  # Pad the story label for alignment.
  printf "  Story %2d (%-40s) " "$n" "$local_name"

  case "$local_result" in
    PASS)
      echo "PASS (${local_duration}s)"
      ((pass_count++)) || true
      ;;
    FAIL)
      echo "FAIL (${local_duration}s) — $local_msg"
      ((fail_count++)) || true
      ;;
    SKIP)
      echo "SKIP — $local_msg"
      ((skip_count++)) || true
      ;;
  esac
done

total=${#STORIES[@]}
echo ""
echo "Summary: $pass_count/$total passed, $fail_count failed, $skip_count skipped"
echo "Logs:    $LOG_DIR/"
echo ""

if [[ $fail_count -gt 0 ]]; then
  exit 1
fi
exit 0
