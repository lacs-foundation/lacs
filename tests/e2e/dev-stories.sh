#!/usr/bin/env bash
# dev-stories.sh — run E2E user stories on your dev machine (no VM required).
#
# What this does:
#   1. Builds lacs-daemon and lacs-test-cli (release mode).
#   2. Starts lacs-daemon in the background on /tmp/lacs-daemon.sock if it is
#      not already running there.
#   3. Runs the requested stories (default: 1-7, the read-only ones).
#   4. Stops the daemon if this script started it.
#
# Stories 1-7 validate plan structure only — they check that the LLM proposes
# the right actions, not that those actions succeed on this machine. They work
# on any Linux host regardless of whether rpm-ostree, flatpak, or podman are
# installed.
#
# Stories 8-10 are destructive (rpm-ostree layering, toolbox creation, SSH key
# writes). They also call query_* tools, and those calls will fail on a non-
# Fedora-Atomic host because the underlying commands are absent. Stories 8 and
# 10 will fail on a dev machine for this reason. Story 9 (create toolbox) may
# pass plan-structure checks. To run them anyway:
#
#   LACS_ALLOW_DESTRUCTIVE=1 tests/e2e/dev-stories.sh 8 9 10
#
# LLM provider is auto-detected (same logic as BrainConfig::from_env):
#   - ANTHROPIC_API_KEY set  → provider=anthropic, model=claude-sonnet-4-6
#   - OPENAI_API_KEY set     → provider=openai,    model=gpt-4o
#   - GEMINI_API_KEY set     → provider=gemini,    model=gemini-2.0-flash
#   - otherwise              → provider=ollama,    model=qwen3:8b (must be pulled)
#
# Override with LACS_LLM_PROVIDER and LACS_LLM_MODEL.
#
# Usage:
#   tests/e2e/dev-stories.sh            # stories 1-7
#   tests/e2e/dev-stories.sh 3 6 7      # specific stories
#   LACS_ALLOW_DESTRUCTIVE=1 tests/e2e/dev-stories.sh   # all 10
#   LACS_LLM_PROVIDER=openai OPENAI_API_KEY=sk-... tests/e2e/dev-stories.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LOG_DIR="$SCRIPT_DIR/logs"
STORY_DIR="$SCRIPT_DIR/stories"
SOCKET_PATH="/tmp/lacs-daemon.sock"
DAEMON_PID=""

mkdir -p "$LOG_DIR"

# ---------------------------------------------------------------------------
# Cleanup — stop the daemon if we started it
# ---------------------------------------------------------------------------

cleanup() {
    if [[ -n "$DAEMON_PID" ]]; then
        echo ""
        echo "Stopping lacs-daemon (pid $DAEMON_PID)..."
        kill "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
        rm -f "$SOCKET_PATH" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

echo "Building lacs-daemon and lacs-test-cli..."
cargo build -p lacs-daemon -p lacs-test-cli --release --quiet \
    --manifest-path "$REPO_ROOT/Cargo.toml"
echo "Build done."
echo ""

DAEMON_BIN="$REPO_ROOT/target/release/lacs-daemon"

# ---------------------------------------------------------------------------
# Start daemon if not already running
# ---------------------------------------------------------------------------

if [[ -e "$SOCKET_PATH" ]]; then
    echo "lacs-daemon socket already present at $SOCKET_PATH — skipping start."
else
    echo "Starting lacs-daemon on $SOCKET_PATH..."
    LACS_LISTEN_URI="unix://$SOCKET_PATH" \
    LACS_DATABASE_PATH="/tmp/lacs-daemon-dev.sqlite" \
        "$DAEMON_BIN" >"$LOG_DIR/daemon.log" 2>&1 &
    DAEMON_PID=$!

    # Wait up to 5 s for the socket to appear.
    local_waited=0
    while [[ ! -e "$SOCKET_PATH" ]] && (( local_waited < 50 )); do
        sleep 0.1
        (( local_waited++ )) || true
    done

    if [[ ! -e "$SOCKET_PATH" ]]; then
        echo "ERROR: lacs-daemon did not start within 5 s."
        echo "Daemon log ($LOG_DIR/daemon.log):"
        cat "$LOG_DIR/daemon.log" || true
        exit 1
    fi
    if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
        echo "ERROR: lacs-daemon process exited before socket appeared."
        echo "Daemon log ($LOG_DIR/daemon.log):"
        cat "$LOG_DIR/daemon.log" || true
        exit 1
    fi
    echo "lacs-daemon started (pid $DAEMON_PID)."
fi
echo ""

# ---------------------------------------------------------------------------
# LLM provider auto-detection
# ---------------------------------------------------------------------------

# Respect explicit override first; then auto-detect from API keys.
if [[ -z "${LACS_LLM_PROVIDER:-}" ]]; then
    if [[ -n "${ANTHROPIC_API_KEY:-}" ]]; then
        export LACS_LLM_PROVIDER="anthropic"
    elif [[ -n "${OPENAI_API_KEY:-}" ]]; then
        export LACS_LLM_PROVIDER="openai"
    elif [[ -n "${GEMINI_API_KEY:-}" ]]; then
        export LACS_LLM_PROVIDER="gemini"
    else
        export LACS_LLM_PROVIDER="ollama"
    fi
fi
export LACS_LISTEN_URI="unix://$SOCKET_PATH"
export PATH="$REPO_ROOT/target/release:$PATH"

echo "LLM provider: $LACS_LLM_PROVIDER, model: ${LACS_LLM_MODEL:-<provider default>}"
echo "Daemon socket: $SOCKET_PATH"
echo ""

# ---------------------------------------------------------------------------
# Story metadata
# ---------------------------------------------------------------------------

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

ALLOW_DESTRUCTIVE="${LACS_ALLOW_DESTRUCTIVE:-0}"
STORY_TIMEOUT="${LACS_STORY_TIMEOUT:-120}"

declare -a STORIES
declare -A RESULTS
declare -A DURATIONS
declare -A MESSAGES

if [[ $# -gt 0 ]]; then
    STORIES=("$@")
elif [[ "$ALLOW_DESTRUCTIVE" == "1" ]]; then
    STORIES=(1 2 3 4 5 6 7 8 9 10)
else
    STORIES=(1 2 3 4 5 6 7)
fi

# ---------------------------------------------------------------------------
# Story runner
# ---------------------------------------------------------------------------

run_story() {
    local n="$1"
    local name="${STORY_NAMES[$n]:-Story $n}"
    local log="$LOG_DIR/story-${n}.log"
    local script="$STORY_DIR/story-${n}.sh"

    printf "Story %2d  %-46s " "$n" "(${name})"

    if [[ ! -f "$script" ]]; then
        RESULTS[$n]="FAIL"
        MESSAGES[$n]="script not found: $script"
        echo "FAIL — script not found"
        return
    fi

    local start_time exit_code
    start_time=$(date +%s.%N)
    set +e
    timeout "$STORY_TIMEOUT" bash "$script" >"$log" 2>&1
    exit_code=$?
    set -e
    local end_time elapsed
    end_time=$(date +%s.%N)
    elapsed=$(awk "BEGIN{printf \"%.1f\", $end_time - $start_time}" 2>/dev/null || echo "?")
    DURATIONS[$n]="$elapsed"

    if [[ $exit_code -eq 0 ]]; then
        # Check the last line for PASS/SKIP markers.
        local last_line
        last_line=$(grep -E '^(PASS|SKIP)' "$log" | tail -1 || true)
        if [[ "$last_line" == SKIP* ]]; then
            RESULTS[$n]="SKIP"
            MESSAGES[$n]="${last_line#SKIP}"
            echo "SKIP"
        else
            RESULTS[$n]="PASS"
            echo "PASS (${elapsed}s)"
        fi
    else
        RESULTS[$n]="FAIL"
        MESSAGES[$n]=$(tail -n 5 "$log" | grep -v '^$' | tail -n 1 || true)
        if [[ $exit_code -eq 124 ]]; then
            MESSAGES[$n]="timed out after ${STORY_TIMEOUT}s"
        fi
        echo "FAIL (${elapsed}s)"
    fi
}

# ---------------------------------------------------------------------------
# Execute
# ---------------------------------------------------------------------------

echo "LACS Dev Story Run"
echo "=================="
echo "Date:        $(date --iso-8601=seconds 2>/dev/null || date)"
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
    local_duration="${DURATIONS[$n]:-?}"
    local_msg="${MESSAGES[$n]:-}"

    printf "  Story %2d  %-46s " "$n" "(${local_name})"
    case "$local_result" in
        PASS)
            echo "PASS (${local_duration}s)"
            (( pass_count++ )) || true
            ;;
        FAIL)
            echo "FAIL (${local_duration}s) — $local_msg"
            (( fail_count++ )) || true
            ;;
        SKIP)
            echo "SKIP$local_msg"
            (( skip_count++ )) || true
            ;;
    esac
done

total=${#STORIES[@]}
echo ""
echo "Summary: $pass_count/$total passed, $fail_count failed, $skip_count skipped"
echo "Logs:    $LOG_DIR/"
echo ""

if (( fail_count > 0 )); then
    echo "NOTE: On a non-Fedora-Atomic host, stories 8 and 10 are expected to fail"
    echo "because query_packages and query_authorized_keys call rpm-ostree and SSH"
    echo "tools that are absent. Stories 1-7 should always pass on any Linux host."
    echo "Run on a provisioned Silverblue VM for full coverage."
    echo ""
    exit 1
fi
exit 0
