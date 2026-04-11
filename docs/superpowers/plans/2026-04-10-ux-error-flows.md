# LACS Shell UX — Error Flows, Approval Gate, and State-Driven Layout

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign lacs-shell's UI with a state-driven layout, a full plan-step display with risk-scaled approval gate, a five-category typed error taxonomy, enriched ExecutionPane with cancel, and a timestamped colour-coded timeline.

**Architecture:** Three phases — (1) data model: TypeScript types, shellReducer, lacs-brain delegation, Tauri commands layer; (2) React components: RiskBadge, ErrorBlock, PlanPane, IntentPane, ExecutionPane, TimelinePane; (3) layout and chrome: App.tsx and styles.css. Each phase compiles and all tests pass before the next begins.

**Tech Stack:** React 18, TypeScript, Vitest + @testing-library/react (frontend); Rust, Tauri v2, serde (Tauri commands); lacs-brain crate (Rust, async).

---

## File map

**Create:**
- `apps/lacs-shell/src/types.ts` — shared TypeScript types mirroring Tauri command responses (`PlanResponse`, `PlanStepResponse`, `ShellError`, `BrainConfigResponse`, `DaemonStatus`)
- `apps/lacs-shell/src/components/RiskBadge.tsx` — risk level badge (low/medium/high)
- `apps/lacs-shell/src/components/ErrorBlock.tsx` — structured three-line error display

**Modify:**
- `apps/lacs-shell/src/shellState.ts` — retire `ShellPreview`, typed `TimelineEntry`, new actions (`plan_ready`, `plan_errored`, `policy_errored`, `cancel_requested`, `daemon_status_changed`), `DaemonStatus` in `Base`, updated `ShellOutcome`
- `apps/lacs-shell/src/shellState.test.ts` — update for new action/type names
- `apps/lacs-shell/src/daemonBridge.ts` — add `getBrainConfig`, `cancelJob`, update `requestPlan` return type
- `apps/lacs-shell/src/App.tsx` — `data-mode` on grid, brain config load, daemon-status dispatch, plan_errored dispatch
- `apps/lacs-shell/src/App.test.tsx` — update mocks and status-text assertions
- `apps/lacs-shell/src/styles.css` — `grid-template-areas` per mode, risk badge colours, timeline entry colours, error block
- `apps/lacs-shell/src/components/IntentPane.tsx` — compact strip vs full form, inline `ErrorBlock`
- `apps/lacs-shell/src/components/PlanPane.tsx` — step breakdown, aggregate risk badge, approval gate
- `apps/lacs-shell/src/components/ExecutionPane.tsx` — step-progress list, cancel, terminal states
- `apps/lacs-shell/src/components/TimelinePane.tsx` — timestamps, colour dots, auto-scroll
- `crates/lacs-brain/src/config.rs` — add `BrainConfig::provider_name()` and `BrainConfig::model_name()`
- `crates/lacs-brain/src/planner.rs` — add `LlmPlanner::curated_state()` delegation
- `apps/lacs-shell/src-tauri/src/commands.rs` — `ShellError` struct, updated `PlanResponse` (system context fields, no `ShellPreview`), updated `plan_intent` and `approve_preview`, new `get_brain_config` and `cancel_job`
- `apps/lacs-shell/src-tauri/src/main.rs` — register `get_brain_config` and `cancel_job`

---

## Test commands

TypeScript (run from `apps/lacs-shell/`):
```bash
npx vitest run --reporter=verbose
```

Single file:
```bash
npx vitest run src/shellState.test.ts --reporter=verbose
```

Rust — Tauri commands:
```bash
cd apps/lacs-shell/src-tauri && cargo test -- --nocapture
```

Rust — lacs-brain:
```bash
cargo test -p lacs-brain -- --nocapture
```

---

## Task 1: Create `src/types.ts` — shared TypeScript types

**Files:**
- Create: `apps/lacs-shell/src/types.ts`

- [ ] **Step 1: Write the failing import test**

Add to `apps/lacs-shell/src/shellState.test.ts` at the top (before existing imports):
```typescript
import type { PlanResponse, ShellError } from "./types";
```

Run:
```bash
cd apps/lacs-shell && npx vitest run src/shellState.test.ts --reporter=verbose
```
Expected: FAIL — `Cannot find module './types'`

- [ ] **Step 2: Create `src/types.ts`**

```typescript
// Tauri response types — mirror the structs in commands.rs exactly.
// All field names are camelCase because commands.rs uses #[serde(rename_all = "camelCase")].

export interface PlanStepResponse {
  actionName: string;
  summary: string;
  riskLevel: "low" | "medium" | "high";
  approvalRequired: boolean;
}

export interface PlanResponse {
  summary: string;
  explanation: string;
  approvalRequired: boolean;
  steps: PlanStepResponse[];
  hostName: string;
  deployment: string;
  toolboxCount: number;
  flatpakCount: number;
}

export interface BrainConfigResponse {
  provider: string;  // "anthropic" | "ollama"
  model: string;     // e.g. "claude-opus-4-6" or "mistral:7b"
  fallback: boolean; // true when BrainConfig::from_env() failed and defaults were used
}

export type ShellErrorCode =
  | "daemon_not_running"
  | "daemon_permission_denied"
  | "llm_rate_limit"
  | "llm_http_error"
  | "llm_parse_error"
  | "safety_fence"
  | "intent_empty"
  | "role_insufficient"
  | "stale_approval"
  | "execution_failed_with_rollback"
  | "execution_failed_no_rollback"
  | "unknown";

export interface ShellError {
  code: ShellErrorCode;
  message: string;       // human-readable detail, never a raw Rust error string
  systemChanged: boolean;
}

export type DaemonStatus = "unknown" | "connected" | "unreachable";
```

- [ ] **Step 3: Run test to verify the import resolves**

```bash
cd apps/lacs-shell && npx vitest run src/shellState.test.ts --reporter=verbose
```
Expected: all existing tests PASS (new import is type-only, no runtime effect)

- [ ] **Step 4: Commit**

```bash
git add apps/lacs-shell/src/types.ts apps/lacs-shell/src/shellState.test.ts
git commit -m "feat(shell): add shared TypeScript types (PlanResponse, ShellError, DaemonStatus)"
```

---

## Task 2: Update `shellState.ts` — types and state shapes

**Files:**
- Modify: `apps/lacs-shell/src/shellState.ts`
- Modify: `apps/lacs-shell/src/shellState.test.ts`

- [ ] **Step 1: Write failing tests for new state shapes**

Replace the entire contents of `apps/lacs-shell/src/shellState.test.ts` with:

```typescript
import type { PlanResponse, ShellError } from "./types";
import {
  initialShellState,
  shellReducer,
  type ShellState,
} from "./shellState";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const MOCK_PLAN: PlanResponse = {
  summary: "Install vim on this machine",
  explanation: "rpm-ostree will layer vim. A reboot will be required.",
  approvalRequired: true,
  steps: [
    { actionName: "GetSystemState", summary: "Read current deployment", riskLevel: "low", approvalRequired: false },
    { actionName: "InstallPackages", summary: "Layer vim via rpm-ostree", riskLevel: "high", approvalRequired: true },
  ],
  hostName: "silverblue",
  deployment: "fedora/41",
  toolboxCount: 1,
  flatpakCount: 2,
};

const MOCK_ERROR: ShellError = {
  code: "llm_http_error",
  message: "HTTP 500 — internal server error",
  systemChanged: false,
};

// Helper: drive to awaiting-approval
function reachAwaitingApproval(): ShellState {
  return shellReducer(
    shellReducer(
      shellReducer(initialShellState, { type: "intent_submitted", intent: "install vim" }),
      { type: "plan_ready", plan: MOCK_PLAN },
    ),
    { type: "request_approval" },
  );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("shellReducer — initial state", () => {
  it("starts in idle mode with daemonStatus unknown", () => {
    expect(initialShellState.mode).toBe("idle");
    expect(initialShellState.daemonStatus).toBe("unknown");
  });
});

describe("shellReducer — happy path", () => {
  it("idle → planning → previewing with full plan", () => {
    const planning = shellReducer(initialShellState, {
      type: "intent_submitted",
      intent: "install vim",
    });
    const previewing = shellReducer(planning, { type: "plan_ready", plan: MOCK_PLAN });

    expect(planning.mode).toBe("planning");
    expect(previewing.mode).toBe("previewing");
    if (previewing.mode === "previewing") {
      expect(previewing.plan.summary).toBe("Install vim on this machine");
      expect(previewing.plan.steps).toHaveLength(2);
    }
  });

  it("previewing → awaiting-approval → executing → succeeded → idle", () => {
    const awaiting = reachAwaitingApproval();
    const executing = shellReducer(awaiting, { type: "approval_granted" });
    const succeeded = shellReducer(executing, { type: "job_completed", outcome: "succeeded" });

    expect(awaiting.mode).toBe("awaiting-approval");
    expect(executing.mode).toBe("executing");
    expect(succeeded.mode).toBe("idle");
    if (succeeded.mode === "idle") {
      expect(succeeded.plan).toBeNull();
    }
  });

  it("executing → needs-reboot", () => {
    const executing = shellReducer(reachAwaitingApproval(), { type: "approval_granted" });
    const needsReboot = shellReducer(executing, { type: "job_completed", outcome: "needs_reboot" });
    expect(needsReboot.mode).toBe("needs-reboot");
  });

  it("executing → rolled-back", () => {
    const executing = shellReducer(reachAwaitingApproval(), { type: "approval_granted" });
    const rolledBack = shellReducer(executing, { type: "job_completed", outcome: "rolled_back" });
    expect(rolledBack.mode).toBe("rolled-back");
    if (rolledBack.mode === "rolled-back") {
      expect(rolledBack.plan).not.toBeNull();
    }
  });

  it("executing → canceled", () => {
    const executing = shellReducer(reachAwaitingApproval(), { type: "approval_granted" });
    const canceled = shellReducer(executing, { type: "job_completed", outcome: "canceled" });
    expect(canceled.mode).toBe("idle");
  });
});

describe("shellReducer — error paths", () => {
  it("plan_errored keeps mode idle and stores error", () => {
    const planning = shellReducer(initialShellState, {
      type: "intent_submitted",
      intent: "install vim",
    });
    const errored = shellReducer(planning, { type: "plan_errored", error: MOCK_ERROR });

    expect(errored.mode).toBe("idle");
    if (errored.mode === "idle") {
      expect(errored.error?.code).toBe("llm_http_error");
    }
  });

  it("intent_submitted clears a previous plan_errored error", () => {
    const withError = shellReducer(
      shellReducer(initialShellState, { type: "intent_submitted", intent: "install vim" }),
      { type: "plan_errored", error: MOCK_ERROR },
    );
    const resubmitted = shellReducer(withError, { type: "intent_submitted", intent: "install neovim" });
    expect(resubmitted.mode).toBe("planning");
    if (resubmitted.mode === "planning") {
      // planning state has no error field — just verify mode changed
      expect(resubmitted.intent).toBe("install neovim");
    }
  });

  it("policy_errored keeps mode previewing and stores planError", () => {
    const previewing = shellReducer(
      shellReducer(initialShellState, { type: "intent_submitted", intent: "install vim" }),
      { type: "plan_ready", plan: MOCK_PLAN },
    );
    const policyErr: ShellError = { code: "role_insufficient", message: "Admin required", systemChanged: false };
    const errored = shellReducer(previewing, { type: "policy_errored", error: policyErr });

    expect(errored.mode).toBe("previewing");
    if (errored.mode === "previewing") {
      expect(errored.planError?.code).toBe("role_insufficient");
    }
  });

  it("job_completed:failed from executing transitions to failed mode", () => {
    const executing = shellReducer(reachAwaitingApproval(), { type: "approval_granted" });
    const failed = shellReducer(executing, { type: "job_completed", outcome: "failed" });

    expect(failed.mode).toBe("failed");
    if (failed.mode === "failed") {
      expect(failed.activeJobId).toBeNull();
      expect(failed.plan).not.toBeNull();
    }
  });
});

describe("shellReducer — daemon status", () => {
  it("daemon_status_changed updates daemonStatus in any mode", () => {
    const updated = shellReducer(initialShellState, {
      type: "daemon_status_changed",
      status: "connected",
    });
    expect(updated.daemonStatus).toBe("connected");
  });
});

describe("shellReducer — timeline entries have timestamp and kind", () => {
  it("intent_submitted appends a user-kind entry with a timestamp", () => {
    const s = shellReducer(initialShellState, { type: "intent_submitted", intent: "install vim" });
    const last = s.timeline[s.timeline.length - 1];
    expect(last.kind).toBe("user");
    expect(last.timestamp).toMatch(/^\d{2}:\d{2}:\d{2}$/);
  });

  it("job_completed:succeeded appends a success-kind entry", () => {
    const executing = shellReducer(reachAwaitingApproval(), { type: "approval_granted" });
    const succeeded = shellReducer(executing, { type: "job_completed", outcome: "succeeded" });
    const last = succeeded.timeline[succeeded.timeline.length - 1];
    expect(last.kind).toBe("success");
  });

  it("plan_errored appends an error-kind entry", () => {
    const planning = shellReducer(initialShellState, { type: "intent_submitted", intent: "x" });
    const errored = shellReducer(planning, { type: "plan_errored", error: MOCK_ERROR });
    const last = errored.timeline[errored.timeline.length - 1];
    expect(last.kind).toBe("error");
  });
});

describe("shellReducer — reset", () => {
  it("reset from failed returns to idle", () => {
    const executing = shellReducer(reachAwaitingApproval(), { type: "approval_granted" });
    const failed = shellReducer(executing, { type: "job_completed", outcome: "failed" });
    const afterReset = shellReducer(failed, { type: "reset" });
    expect(afterReset.mode).toBe("idle");
    expect(afterReset.intent).toBe("");
  });
});
```

Run:
```bash
cd apps/lacs-shell && npx vitest run src/shellState.test.ts --reporter=verbose
```
Expected: multiple FAILs — `plan_ready` unknown, `daemonStatus` missing, `plan` missing, `plan_errored` unknown, etc.

- [ ] **Step 2: Rewrite `shellState.ts`**

Replace the entire file:

```typescript
import type { DaemonStatus, PlanResponse, ShellError } from "./types";

export type { DaemonStatus, ShellError } from "./types";

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

export type TimelineEntryKind =
  | "system"   // state transitions, daemon events
  | "user"     // explicit user actions
  | "success"  // completions
  | "warning"  // reboot required, rollbacks, cancellations
  | "error";   // failures, policy denials

export interface TimelineEntry {
  id: string;
  timestamp: string;  // HH:MM:SS wall-clock, set in appendTimeline
  kind: TimelineEntryKind;
  text: string;
}

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

export type ShellOutcome =
  | "succeeded"
  | "needs_reboot"
  | "failed"      // execution failure → transitions to "failed" mode
  | "rolled_back"
  | "canceled";

// ---------------------------------------------------------------------------
// State — discriminated union
//
// Invariants:
//   plan is non-null iff mode is previewing/awaiting-approval/executing/needs-reboot/rolled-back/failed
//   activeJobId is non-null only in executing
//   error is non-null only in idle (planning / pre-flight failures)
//   planError is non-null only in previewing / awaiting-approval (policy errors)
// ---------------------------------------------------------------------------

type Base = {
  intent: string;
  timeline: TimelineEntry[];
  daemonStatus: DaemonStatus;
};

type IdleState = Base & {
  mode: "idle";
  plan: null;
  activeJobId: null;
  error: ShellError | null;
};

type PlanningState = Base & {
  mode: "planning";
  plan: null;
  activeJobId: null;
};

type PreviewingState = Base & {
  mode: "previewing";
  plan: PlanResponse;
  activeJobId: null;
  planError: ShellError | null;
};

type ApprovingState = Base & {
  mode: "awaiting-approval";
  plan: PlanResponse;
  activeJobId: null;
  planError: ShellError | null;
};

type ExecutingState = Base & {
  mode: "executing";
  plan: PlanResponse;
  activeJobId: string;
};

type NeedsRebootState = Base & {
  mode: "needs-reboot";
  plan: PlanResponse;
  activeJobId: null;
};

// failed is only reached from execution — plan is always present.
type FailedState = Base & {
  mode: "failed";
  plan: PlanResponse;
  activeJobId: null;
};

// rolled-back is only reached from execution — plan is always present.
type RolledBackState = Base & {
  mode: "rolled-back";
  plan: PlanResponse;
  activeJobId: null;
};

export type ShellState =
  | IdleState
  | PlanningState
  | PreviewingState
  | ApprovingState
  | ExecutingState
  | NeedsRebootState
  | FailedState
  | RolledBackState;

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

export type ShellAction =
  | { type: "intent_submitted"; intent: string }
  | { type: "plan_ready"; plan: PlanResponse }
  | { type: "request_approval" }
  | { type: "approval_granted" }
  | { type: "job_completed"; outcome: ShellOutcome }
  | { type: "timeline_event"; text: string; kind: TimelineEntryKind }
  | { type: "plan_errored"; error: ShellError }       // categories 1–2: stays idle
  | { type: "policy_errored"; error: ShellError }     // category 3: stays previewing/approving
  | { type: "cancel_requested" }
  | { type: "daemon_status_changed"; status: DaemonStatus }
  | { type: "reset" };

// ---------------------------------------------------------------------------
// Initial state
// ---------------------------------------------------------------------------

export const initialShellState: ShellState = {
  mode: "idle",
  intent: "",
  plan: null,
  activeJobId: null,
  error: null,
  daemonStatus: "unknown",
  timeline: [],
};

// ---------------------------------------------------------------------------
// Reducer
// ---------------------------------------------------------------------------

export function shellReducer(state: ShellState, action: ShellAction): ShellState {
  switch (action.type) {
    case "intent_submitted": {
      const next: PlanningState = {
        mode: "planning",
        intent: action.intent,
        plan: null,
        activeJobId: null,
        daemonStatus: state.daemonStatus,
        timeline: state.timeline,
      };
      return appendTimeline(next, `Intent submitted: ${action.intent}`, "user");
    }

    case "plan_ready": {
      const next: PreviewingState = {
        mode: "previewing",
        intent: state.intent,
        plan: action.plan,
        activeJobId: null,
        planError: null,
        daemonStatus: state.daemonStatus,
        timeline: state.timeline,
      };
      return appendTimeline(next, `Plan ready: ${action.plan.summary}`, "system");
    }

    case "request_approval": {
      if (state.mode !== "previewing") return state;
      const next: ApprovingState = {
        mode: "awaiting-approval",
        intent: state.intent,
        plan: state.plan,
        activeJobId: null,
        planError: null,
        daemonStatus: state.daemonStatus,
        timeline: state.timeline,
      };
      return appendTimeline(next, "Awaiting user approval", "system");
    }

    case "approval_granted": {
      if (state.mode !== "awaiting-approval") return state;
      const next: ExecutingState = {
        mode: "executing",
        intent: state.intent,
        plan: state.plan,
        // Real job ID arrives from the daemon's ResultEnvelope after approve_preview.
        // TODO(daemon-ipc): replace synthesised ID with the daemon's job ID.
        activeJobId: `job-${state.timeline.length + 1}`,
        daemonStatus: state.daemonStatus,
        timeline: state.timeline,
      };
      return appendTimeline(next, "Approval granted — executing", "user");
    }

    case "job_completed": {
      const { outcome } = action;

      if (outcome === "succeeded") {
        const next: IdleState = {
          mode: "idle",
          intent: state.intent,
          plan: null,
          activeJobId: null,
          error: null,
          daemonStatus: state.daemonStatus,
          timeline: state.timeline,
        };
        return appendTimeline(next, "Job completed successfully", "success");
      }

      if (outcome === "needs_reboot") {
        const plan = (state as ExecutingState).plan;
        const next: NeedsRebootState = {
          mode: "needs-reboot",
          intent: state.intent,
          plan,
          activeJobId: null,
          daemonStatus: state.daemonStatus,
          timeline: state.timeline,
        };
        return appendTimeline(next, "Job completed — reboot required", "warning");
      }

      if (outcome === "rolled_back") {
        const plan = (state as ExecutingState).plan;
        const next: RolledBackState = {
          mode: "rolled-back",
          intent: state.intent,
          plan,
          activeJobId: null,
          daemonStatus: state.daemonStatus,
          timeline: state.timeline,
        };
        return appendTimeline(next, "Job rolled back", "warning");
      }

      if (outcome === "canceled") {
        const next: IdleState = {
          mode: "idle",
          intent: state.intent,
          plan: null,
          activeJobId: null,
          error: null,
          daemonStatus: state.daemonStatus,
          timeline: state.timeline,
        };
        return appendTimeline(next, "Job canceled", "warning");
      }

      // "failed" — execution failure; plan must be present
      const plan = (state as ExecutingState).plan;
      const next: FailedState = {
        mode: "failed",
        intent: state.intent,
        plan,
        activeJobId: null,
        daemonStatus: state.daemonStatus,
        timeline: state.timeline,
      };
      return appendTimeline(next, "Job failed", "error");
    }

    case "plan_errored": {
      // Pre-flight or planning failure — stays idle, error shown inline in IntentPane.
      const next: IdleState = {
        mode: "idle",
        intent: state.intent,
        plan: null,
        activeJobId: null,
        error: action.error,
        daemonStatus: state.daemonStatus,
        timeline: state.timeline,
      };
      return appendTimeline(next, `Planning failed: ${action.error.message}`, "error");
    }

    case "policy_errored": {
      // Policy / stale-approval failure — stays previewing or awaiting-approval.
      if (state.mode !== "previewing" && state.mode !== "awaiting-approval") return state;
      const next = {
        ...state,
        planError: action.error,
      } as PreviewingState | ApprovingState;
      return appendTimeline(next, `Policy error: ${action.error.message}`, "error");
    }

    case "cancel_requested": {
      return appendTimeline(state, "Cancellation requested", "user");
    }

    case "daemon_status_changed": {
      return { ...state, daemonStatus: action.status };
    }

    case "timeline_event": {
      return appendTimeline(state, action.text, action.kind);
    }

    case "reset": {
      return initialShellState;
    }

    default: {
      const exhaustiveCheck: never = action;
      console.warn("[LACS] shellReducer received unknown action:", exhaustiveCheck);
      return state;
    }
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function appendTimeline<S extends ShellState>(
  state: S,
  text: string,
  kind: TimelineEntryKind,
): S {
  const timestamp = new Date().toLocaleTimeString("en-US", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
  return {
    ...state,
    timeline: [
      ...state.timeline,
      { id: String(state.timeline.length + 1), timestamp, kind, text },
    ],
  } as S;
}
```

- [ ] **Step 3: Run tests**

```bash
cd apps/lacs-shell && npx vitest run src/shellState.test.ts --reporter=verbose
```
Expected: all tests PASS

- [ ] **Step 4: Commit**

```bash
git add apps/lacs-shell/src/shellState.ts apps/lacs-shell/src/shellState.test.ts
git commit -m "feat(shell): update shellState — typed timeline, plan_ready, plan_errored, DaemonStatus"
```

---

## Task 3: Add `BrainConfig` accessor methods and `LlmPlanner::curated_state()`

**Files:**
- Modify: `crates/lacs-brain/src/config.rs`
- Modify: `crates/lacs-brain/src/planner.rs`

- [ ] **Step 1: Write failing tests in `config.rs`**

Add to the test module at the bottom of `crates/lacs-brain/src/config.rs`:

```rust
#[test]
fn provider_name_and_model_name_anthropic() {
    let cfg = BrainConfig {
        provider: ProviderConfig::Anthropic {
            api_key: "sk-test".into(),
            model: "claude-opus-4-6".into(),
            base_url: "https://api.anthropic.com".into(),
        },
        max_turns: 5,
    };
    assert_eq!(cfg.provider_name(), "anthropic");
    assert_eq!(cfg.model_name(), "claude-opus-4-6");
}

#[test]
fn provider_name_and_model_name_ollama() {
    let cfg = BrainConfig::ollama_defaults();
    assert_eq!(cfg.provider_name(), "ollama");
    // model_name() is non-empty for ollama defaults
    assert!(!cfg.model_name().is_empty());
}
```

Run:
```bash
cargo test -p lacs-brain -- --nocapture 2>&1 | grep -E "FAILED|error|provider_name"
```
Expected: FAIL — `no method named provider_name`

- [ ] **Step 2: Add `provider_name()` and `model_name()` to `BrainConfig`**

In `crates/lacs-brain/src/config.rs`, add after the `impl fmt::Debug for BrainConfig` block:

```rust
impl BrainConfig {
    /// Returns `"anthropic"` or `"ollama"`.
    pub fn provider_name(&self) -> &str {
        match &self.provider {
            ProviderConfig::Anthropic { .. } => "anthropic",
            ProviderConfig::Ollama { .. } => "ollama",
        }
    }

    /// Returns the model identifier string (e.g. `"claude-opus-4-6"` or `"mistral:7b"`).
    pub fn model_name(&self) -> &str {
        match &self.provider {
            ProviderConfig::Anthropic { model, .. } => model,
            ProviderConfig::Ollama { model, .. } => model,
        }
    }
}
```

- [ ] **Step 3: Run config tests**

```bash
cargo test -p lacs-brain -- --nocapture 2>&1 | grep -E "FAILED|ok|provider_name|model_name"
```
Expected: `test ... provider_name_and_model_name_anthropic ... ok` and `... ollama ... ok`

- [ ] **Step 4: Write failing test for `LlmPlanner::curated_state()`**

Add to the test module in `crates/lacs-brain/src/planner.rs` (already has test helpers — add after existing tests):

```rust
#[tokio::test]
async fn curated_state_delegates_to_state_client() {
    // The DemoStateClient fixture returns a known hostname.
    // LlmPlanner::curated_state() must return the same value.
    let planner = LlmPlanner::new(
        Box::new(MockProvider::once(Err(ProviderError::Parse("unused".into())))),
        Box::new(DemoStateClient),
        1,
    );
    let state = planner.curated_state().expect("DemoStateClient must not fail");
    assert_eq!(state.host_name, "silverblue");
}
```

Run:
```bash
cargo test -p lacs-brain curated_state_delegates -- --nocapture
```
Expected: FAIL — `no method named curated_state`

- [ ] **Step 5: Add `curated_state()` to `LlmPlanner`**

In `crates/lacs-brain/src/planner.rs`, add inside `impl LlmPlanner` after `plan_intent`:

```rust
/// Expose the current system state from the underlying `StateClient`.
///
/// Used by the Tauri commands layer to populate system-context fields in
/// `PlanResponse` without requiring a second network call.
pub fn curated_state(&self) -> Result<crate::state_client::CuratedState, PlanningError> {
    self.state_client.curated_state()
}
```

- [ ] **Step 6: Run all lacs-brain tests**

```bash
cargo test -p lacs-brain -- --nocapture 2>&1 | tail -5
```
Expected: `test result: ok. N passed; 0 failed`

- [ ] **Step 7: Commit**

```bash
git add crates/lacs-brain/src/config.rs crates/lacs-brain/src/planner.rs
git commit -m "feat(brain): add BrainConfig::provider_name/model_name and LlmPlanner::curated_state"
```

---

## Task 4: Update `commands.rs` — `PlanResponse`, `ShellError`, `get_brain_config`, `cancel_job`

**Files:**
- Modify: `apps/lacs-shell/src-tauri/src/commands.rs`
- Modify: `apps/lacs-shell/src-tauri/src/main.rs`

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)]` module at the bottom of `commands.rs`:

```rust
#[test]
fn plan_response_includes_system_context() {
    use lacs_brain::planner::{Plan, PlanStep, PlanRiskLevel};
    let step = PlanStep::new(
        "GetSystemState".into(),
        "Read state".into(),
        PlanRiskLevel::Low,
        serde_json::json!({}),
    );
    let plan = Plan::new(
        "intent".into(),
        "Read the system".into(),
        "Explanation.".into(),
        vec![step],
    );
    // DemoStateClient returns host_name = "silverblue"
    let curated = DemoStateClient.curated_state().unwrap();
    let resp = plan_to_response(plan, &curated);
    assert_eq!(resp.host_name, "silverblue");
    assert_eq!(resp.deployment, "fedora/41");
    assert_eq!(resp.toolbox_count, 1);
    assert_eq!(resp.flatpak_count, 1);
    // preview field must not exist (ShellPreview retired)
    assert!(resp.approval_required == false);
}

#[test]
fn get_brain_config_returns_provider_and_model() {
    let state = ShellCommandState::new();
    let cfg = state.brain_config_response();
    // provider must be "anthropic" or "ollama"
    assert!(cfg.provider == "anthropic" || cfg.provider == "ollama");
    assert!(!cfg.model.is_empty());
}

#[tokio::test]
async fn plan_errored_on_empty_intent() {
    let planner = LlmPlanner::new(
        Box::new(MockProvider::once(Err(ProviderError::Parse("unused".into())))),
        Box::new(DemoStateClient),
        5,
    );
    let state = ShellCommandState::with_planner(planner, BrainConfigResponse {
        provider: "test".into(),
        model: "test-model".into(),
        fallback: false,
    });
    let err = execute_plan_intent(&state, "").await.unwrap_err();
    assert_eq!(err.code, "intent_empty");
    assert!(!err.system_changed);
}
```

Run:
```bash
cd apps/lacs-shell/src-tauri && cargo test -- --nocapture 2>&1 | grep -E "FAILED|error\[" | head -20
```
Expected: FAIL — `host_name` not found, `brain_config_response` not found, `ShellError` not found.

- [ ] **Step 2: Rewrite `commands.rs`**

Replace the full file content. The key structural changes:
1. Remove `ShellPreview` struct
2. Add `ShellError` and `BrainConfigResponse` structs
3. Add `host_name`, `deployment`, `toolbox_count`, `flatpak_count` to `PlanResponse`
4. `plan_to_response` takes a `&CuratedState` second argument
5. `execute_plan_intent` calls `curated_state()` before planning and returns `Result<PlanResponse, ShellError>`
6. `ShellCommandState` stores `brain_config: BrainConfigResponse`
7. New commands: `get_brain_config`, `cancel_job`

```rust
use crate::events::{DaemonJobOutcome, TimelineEvent};
use lacs_brain::config::BrainConfig;
#[cfg(any(test, feature = "demo"))]
use lacs_brain::planner::PlanningError;
use lacs_brain::planner::{LlmPlanner, Plan};
#[cfg(any(test, feature = "demo"))]
use lacs_brain::state_client::{CuratedState, StateClient};
use lacs_brain::state_client::CuratedState;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

// ---------------------------------------------------------------------------
// Response types (serialised to the frontend)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStepResponse {
    pub action_name: String,
    pub summary: String,
    pub risk_level: String,
    pub approval_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanResponse {
    pub summary: String,
    pub explanation: String,
    pub approval_required: bool,
    pub steps: Vec<PlanStepResponse>,
    pub host_name: String,
    pub deployment: String,
    pub toolbox_count: usize,
    pub flatpak_count: usize,
}

/// Typed error returned to the frontend. `code` matches `ShellErrorCode` in `types.ts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellError {
    pub code: String,
    pub message: String,
    pub system_changed: bool,
}

impl ShellError {
    fn pre_flight(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), system_changed: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainConfigResponse {
    pub provider: String,
    pub model: String,
    pub fallback: bool,
}

// ---------------------------------------------------------------------------
// Demo state client (hardcoded Silverblue fixture)
// ---------------------------------------------------------------------------

#[cfg(any(test, feature = "demo"))]
#[derive(Clone, Debug, Default)]
pub struct DemoStateClient;

#[cfg(any(test, feature = "demo"))]
impl StateClient for DemoStateClient {
    fn curated_state(&self) -> Result<CuratedState, PlanningError> {
        Ok(CuratedState {
            host_name: "silverblue".to_string(),
            deployment: "fedora/41".to_string(),
            services: vec!["NetworkManager.service".to_string()],
            flatpaks: vec!["org.mozilla.firefox".to_string()],
            toolboxes: vec!["lacs-dev".to_string()],
        })
    }
}

// ---------------------------------------------------------------------------
// Shell command state
// ---------------------------------------------------------------------------

#[cfg(any(test, feature = "demo"))]
fn build_state_client() -> Box<dyn StateClient> {
    Box::new(DemoStateClient)
}

#[cfg(not(any(test, feature = "demo")))]
fn build_state_client() -> Box<dyn StateClient> {
    panic!("No StateClient available: enable the 'demo' feature or implement a real daemon IPC client")
}

pub struct ShellCommandState {
    planner: LlmPlanner,
    brain_config: BrainConfigResponse,
}

impl ShellCommandState {
    pub fn new() -> Self {
        let env_result = BrainConfig::from_env();
        let fallback = env_result.is_err();
        let config = env_result.unwrap_or_else(|err| {
            eprintln!("[LACS WARNING] Brain config error: {err}. Falling back to Ollama defaults.");
            BrainConfig::ollama_defaults()
        });
        let brain_config = BrainConfigResponse {
            provider: config.provider_name().to_string(),
            model: config.model_name().to_string(),
            fallback,
        };
        let planner = LlmPlanner::from_config(config, build_state_client()).unwrap_or_else(|err| {
            eprintln!(
                "[LACS WARNING] Failed to build LLM provider: {err}. \
                 Check LACS_LLM_PROVIDER and related env vars."
            );
            LlmPlanner::from_config(BrainConfig::ollama_defaults(), build_state_client())
                .expect("Ollama defaults must always produce a valid planner")
        });
        Self { planner, brain_config }
    }

    pub fn brain_config_response(&self) -> BrainConfigResponse {
        self.brain_config.clone()
    }

    #[cfg(test)]
    pub fn with_planner(planner: LlmPlanner, brain_config: BrainConfigResponse) -> Self {
        Self { planner, brain_config }
    }
}

impl Default for ShellCommandState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn plan_intent(
    state: tauri::State<'_, ShellCommandState>,
    intent: String,
) -> Result<PlanResponse, ShellError> {
    execute_plan_intent(&state, &intent).await
}

#[tauri::command]
pub fn approve_preview(app: AppHandle, request_hash: String) -> Result<(), ShellError> {
    // TODO(daemon-ipc): Forward to daemon over Unix socket.
    app.emit("lacs:approval-granted", request_hash)
        .map_err(|err| ShellError::pre_flight("unknown", err.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn get_brain_config(
    state: tauri::State<'_, ShellCommandState>,
) -> BrainConfigResponse {
    state.brain_config_response()
}

#[tauri::command]
pub fn cancel_job(app: AppHandle, job_id: String) -> Result<(), ShellError> {
    // TODO(daemon-ipc): Forward cancellation to daemon over Unix socket.
    app.emit("lacs:job-canceled", job_id)
        .map_err(|err| ShellError::pre_flight("unknown", err.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn publish_timeline_event(app: AppHandle, text: String) -> Result<(), String> {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_nanos()
        .to_string();
    app.emit("lacs:timeline-entry", TimelineEvent { id, text })
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn publish_job_outcome(app: AppHandle, outcome: DaemonJobOutcome) -> Result<(), String> {
    app.emit("lacs:job-completed", outcome)
        .map_err(|err| err.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

pub(crate) async fn execute_plan_intent(
    state: &ShellCommandState,
    intent: &str,
) -> Result<PlanResponse, ShellError> {
    if intent.is_empty() {
        return Err(ShellError::pre_flight("intent_empty", "Intent is empty"));
    }

    let curated = state
        .planner
        .curated_state()
        .map_err(|e| ShellError::pre_flight("unknown", e.to_string()))?;

    let plan = state
        .planner
        .plan_intent(intent)
        .await
        .map_err(map_planning_error)?;

    Ok(plan_to_response(plan, &curated))
}

fn map_planning_error(err: lacs_brain::planner::PlanningError) -> ShellError {
    use lacs_brain::planner::PlanningError;
    let (code, msg) = match &err {
        PlanningError::EmptyIntent => ("intent_empty", err.to_string()),
        PlanningError::StateUnavailable(_) => ("daemon_not_running", err.to_string()),
        PlanningError::Provider(s) => {
            // Surface HTTP status code if present in the provider error string.
            if s.contains("429") {
                ("llm_rate_limit", err.to_string())
            } else if s.starts_with("http") || s.contains("HTTP") {
                ("llm_http_error", err.to_string())
            } else {
                ("llm_parse_error", err.to_string())
            }
        }
        PlanningError::InvalidPlanOutput(_) => ("llm_parse_error", err.to_string()),
        _ => ("unknown", err.to_string()),
    };
    ShellError::pre_flight(code, msg)
}

fn plan_to_response(plan: Plan, curated: &CuratedState) -> PlanResponse {
    let approval_required = plan.steps().iter().any(|step| step.approval_required());
    let steps = plan
        .steps()
        .iter()
        .map(|step| PlanStepResponse {
            action_name: step.action_name().to_string(),
            summary: step.summary().to_string(),
            risk_level: step.risk_level().as_str().to_string(),
            approval_required: step.approval_required(),
        })
        .collect();

    PlanResponse {
        summary: plan.summary().to_string(),
        explanation: plan.explanation().to_string(),
        approval_required,
        steps,
        host_name: curated.host_name.clone(),
        deployment: curated.deployment.clone(),
        toolbox_count: curated.toolboxes.len(),
        flatpak_count: curated.flatpaks.len(),
    }
}
```

- [ ] **Step 3: Register new commands in `main.rs`**

In `apps/lacs-shell/src-tauri/src/main.rs`, update the `use` import and `invoke_handler`:

```rust
use commands::{
    approve_preview, cancel_job, get_brain_config, plan_intent,
    publish_job_outcome, publish_timeline_event, ShellCommandState,
};
```

```rust
.invoke_handler(tauri::generate_handler![
    approve_preview,
    cancel_job,
    get_brain_config,
    plan_intent,
    publish_job_outcome,
    publish_timeline_event
])
```

- [ ] **Step 4: Run Rust tests**

```bash
cd apps/lacs-shell/src-tauri && cargo test -- --nocapture 2>&1 | tail -10
```
Expected: `test result: ok. N passed; 0 failed`

> **Note:** Several existing tests in `commands.rs` reference the old `ShellCommandState::with_planner(planner)` signature. Update them to `ShellCommandState::with_planner(planner, BrainConfigResponse { provider: "test".into(), model: "test".into(), fallback: false })`.

- [ ] **Step 5: Commit**

```bash
git add apps/lacs-shell/src-tauri/src/commands.rs apps/lacs-shell/src-tauri/src/main.rs
git commit -m "feat(shell): add ShellError, system-context PlanResponse, get_brain_config, cancel_job"
```

---

## Task 5: Update `daemonBridge.ts`

**Files:**
- Modify: `apps/lacs-shell/src/daemonBridge.ts`

- [ ] **Step 1: Replace `daemonBridge.ts`**

```typescript
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { BrainConfigResponse, DaemonStatus, PlanResponse, ShellError, TimelineEntryKind } from "./types";
import type { ShellOutcome, TimelineEntry } from "./shellState";

// ---------------------------------------------------------------------------
// Bridge functions
// ---------------------------------------------------------------------------

export async function requestPlan(intent: string): Promise<PlanResponse> {
  requireTauriRuntime();
  return invoke<PlanResponse>("plan_intent", { intent });
}

export async function requestApproval(requestHash: string): Promise<void> {
  requireTauriRuntime();
  await invoke("approve_preview", { requestHash });
}

export async function cancelJob(jobId: string): Promise<void> {
  requireTauriRuntime();
  await invoke("cancel_job", { jobId });
}

export async function getBrainConfig(): Promise<BrainConfigResponse> {
  requireTauriRuntime();
  return invoke<BrainConfigResponse>("get_brain_config");
}

export async function subscribeDaemonEvents(
  onTimeline: (payload: TimelineEntry) => void,
  onOutcome: (payload: ShellOutcome) => void,
): Promise<() => void> {
  requireTauriRuntime();

  const timelineUnlisten = await listen<{ id: string; text: string }>(
    "lacs:timeline-entry",
    (event) => {
      onTimeline({
        id: event.payload.id,
        timestamp: new Date().toLocaleTimeString("en-US", {
          hour12: false,
          hour: "2-digit",
          minute: "2-digit",
          second: "2-digit",
        }),
        kind: "system" as TimelineEntryKind,
        text: event.payload.text,
      });
    },
  );

  const outcomeUnlisten = await listen<ShellOutcome>("lacs:job-completed", (event) => {
    onOutcome(event.payload);
  });

  return () => {
    timelineUnlisten();
    outcomeUnlisten();
  };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function requireTauriRuntime(): void {
  if (!isTauriRuntime()) {
    throw new Error(
      "LACS Shell is not running inside a Tauri runtime. The daemon bridge is unavailable.",
    );
  }
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window;
}

// ---------------------------------------------------------------------------
// Tauri invoke wrapper that returns the daemon status for each call.
// Not exported — used by App.tsx wrappers that dispatch daemon_status_changed.
// ---------------------------------------------------------------------------

export async function withDaemonStatus<T>(
  fn: () => Promise<T>,
): Promise<{ result: T; status: DaemonStatus }> {
  try {
    const result = await fn();
    return { result, status: "connected" };
  } catch {
    return { result: undefined as never, status: "unreachable" };
  }
}
```

> **Note:** The `lacs:preview-ready` event listener is removed — previews now come from the `plan_intent` command response directly, not a separate event. If the daemon later emits preview events over the IPC socket, a new listener can be added at that point.

- [ ] **Step 2: Verify TypeScript compiles**

```bash
cd apps/lacs-shell && npx tsc --noEmit
```
Expected: no errors (some errors may come from App.tsx — those are fixed in Task 9)

- [ ] **Step 3: Commit**

```bash
git add apps/lacs-shell/src/daemonBridge.ts
git commit -m "feat(shell): update daemonBridge — getBrainConfig, cancelJob, typed events"
```

---

## Task 6: Create `RiskBadge` and `ErrorBlock` components

**Files:**
- Create: `apps/lacs-shell/src/components/RiskBadge.tsx`
- Create: `apps/lacs-shell/src/components/ErrorBlock.tsx`

- [ ] **Step 1: Write failing tests for `RiskBadge`**

Create `apps/lacs-shell/src/components/RiskBadge.test.tsx`:

```typescript
import { render, screen } from "@testing-library/react";
import { RiskBadge } from "./RiskBadge";

describe("RiskBadge", () => {
  it("renders LOW with green text", () => {
    render(<RiskBadge level="low" />);
    const badge = screen.getByLabelText("low risk");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveStyle({ color: "#4ade80" });
  });

  it("renders MEDIUM with amber text", () => {
    render(<RiskBadge level="medium" />);
    expect(screen.getByLabelText("medium risk")).toHaveStyle({ color: "#fb923c" });
  });

  it("renders HIGH with red text", () => {
    render(<RiskBadge level="high" />);
    expect(screen.getByLabelText("high risk")).toHaveStyle({ color: "#f87171" });
  });
});
```

Run:
```bash
cd apps/lacs-shell && npx vitest run src/components/RiskBadge.test.tsx --reporter=verbose
```
Expected: FAIL — `Cannot find module './RiskBadge'`

- [ ] **Step 2: Create `RiskBadge.tsx`**

```typescript
import type React from "react";

interface Props {
  level: "low" | "medium" | "high";
}

const STYLES: Record<"low" | "medium" | "high", React.CSSProperties> = {
  low:    { background: "#166534", color: "#4ade80" },
  medium: { background: "#7c2d12", color: "#fb923c" },
  high:   { background: "#7f1d1d", color: "#f87171" },
};

export function RiskBadge({ level }: Props) {
  return (
    <span className="risk-badge" style={STYLES[level]} aria-label={`${level} risk`}>
      ● {level.toUpperCase()}
    </span>
  );
}
```

- [ ] **Step 3: Write failing tests for `ErrorBlock`**

Create `apps/lacs-shell/src/components/ErrorBlock.test.tsx`:

```typescript
import { render, screen } from "@testing-library/react";
import { ErrorBlock } from "./ErrorBlock";
import type { ShellError } from "../types";

const daemonError: ShellError = {
  code: "daemon_not_running",
  message: "unix:///run/lacs/daemon.sock is not available",
  systemChanged: false,
};

const executionError: ShellError = {
  code: "execution_failed_no_rollback",
  message: "Step 2/3 failed — ConfigureService exited with code 1",
  systemChanged: true,
};

describe("ErrorBlock", () => {
  it("renders the title for daemon_not_running", () => {
    render(<ErrorBlock error={daemonError} />);
    expect(screen.getByText(/Cannot reach the LACS daemon/)).toBeInTheDocument();
  });

  it("renders 'Nothing has changed' when systemChanged is false", () => {
    render(<ErrorBlock error={daemonError} />);
    expect(screen.getByText(/Nothing has changed/)).toBeInTheDocument();
  });

  it("renders a warning when systemChanged is true", () => {
    render(<ErrorBlock error={executionError} />);
    expect(screen.getByText(/Some changes cannot be automatically reversed/)).toBeInTheDocument();
  });

  it("renders the error message detail", () => {
    render(<ErrorBlock error={daemonError} />);
    expect(screen.getByText(/unix:\/\/\/run\/lacs\/daemon\.sock/)).toBeInTheDocument();
  });

  it("calls onRetry when Retry button is clicked", async () => {
    const onRetry = vi.fn();
    render(<ErrorBlock error={daemonError} onRetry={onRetry} />);
    screen.getByRole("button", { name: /retry/i }).click();
    expect(onRetry).toHaveBeenCalledOnce();
  });
});
```

Run:
```bash
cd apps/lacs-shell && npx vitest run src/components/ErrorBlock.test.tsx --reporter=verbose
```
Expected: FAIL — `Cannot find module './ErrorBlock'`

- [ ] **Step 4: Create `ErrorBlock.tsx`**

```typescript
import type { ShellError, ShellErrorCode } from "../types";

interface Props {
  error: ShellError;
  onRetry?: () => void;
  onReset?: () => void;
}

const TITLES: Record<ShellErrorCode, string> = {
  daemon_not_running:              "Cannot reach the LACS daemon",
  daemon_permission_denied:        "Permission denied on daemon socket",
  llm_rate_limit:                  "Could not generate a plan (rate limit)",
  llm_http_error:                  "Could not generate a plan",
  llm_parse_error:                 "Could not parse the plan",
  safety_fence:                    "Action blocked by safety policy",
  intent_empty:                    "Intent is empty",
  role_insufficient:               "Insufficient permissions",
  stale_approval:                  "Approval expired",
  execution_failed_with_rollback:  "Execution failed",
  execution_failed_no_rollback:    "Execution failed",
  unknown:                         "An unexpected error occurred",
};

export function ErrorBlock({ error, onRetry, onReset }: Props) {
  const title = TITLES[error.code] ?? TITLES.unknown;
  const showRetry = onRetry && !error.systemChanged;
  const showReset = onReset;

  return (
    <div className="error-block" role="alert">
      <div className="error-block__header">
        <span className="error-block__icon" aria-hidden>⛔</span>
        <strong className="error-block__title">{title}</strong>
      </div>

      <p className="error-block__message">{error.message}</p>

      {error.systemChanged ? (
        <p className="error-block__state error-block__state--warning">
          ⚠ Some changes cannot be automatically reversed. Review the timeline and restore manually if needed.
        </p>
      ) : (
        <p className="error-block__state">Nothing has changed.</p>
      )}

      <div className="error-block__actions">
        {showRetry && (
          <button type="button" onClick={onRetry}>
            Retry
          </button>
        )}
        {showReset && (
          <button type="button" onClick={onReset}>
            New task
          </button>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 5: Run both component tests**

```bash
cd apps/lacs-shell && npx vitest run src/components/RiskBadge.test.tsx src/components/ErrorBlock.test.tsx --reporter=verbose
```
Expected: all PASS

- [ ] **Step 6: Commit**

```bash
git add apps/lacs-shell/src/components/RiskBadge.tsx apps/lacs-shell/src/components/RiskBadge.test.tsx \
        apps/lacs-shell/src/components/ErrorBlock.tsx apps/lacs-shell/src/components/ErrorBlock.test.tsx
git commit -m "feat(shell): add RiskBadge and ErrorBlock components"
```

---

## Task 7: Update `PlanPane` — step breakdown and approval gate

**Files:**
- Modify: `apps/lacs-shell/src/components/PlanPane.tsx`

- [ ] **Step 1: Write failing tests**

Create `apps/lacs-shell/src/components/PlanPane.test.tsx`:

```typescript
import { render, screen, fireEvent } from "@testing-library/react";
import { PlanPane } from "./PlanPane";
import type { PlanResponse, ShellError } from "../types";

const LOW_PLAN: PlanResponse = {
  summary: "Read the system state",
  explanation: "Inspects the current deployment.",
  approvalRequired: false,
  steps: [
    { actionName: "GetSystemState", summary: "Read state", riskLevel: "low", approvalRequired: false },
  ],
  hostName: "silverblue", deployment: "fedora/41", toolboxCount: 1, flatpakCount: 2,
};

const HIGH_PLAN: PlanResponse = {
  summary: "Install vim",
  explanation: "Layers vim via rpm-ostree.",
  approvalRequired: true,
  steps: [
    { actionName: "GetSystemState", summary: "Read state", riskLevel: "low", approvalRequired: false },
    { actionName: "InstallPackages", summary: "Layer vim", riskLevel: "high", approvalRequired: true },
  ],
  hostName: "silverblue", deployment: "fedora/41", toolboxCount: 0, flatpakCount: 0,
};

describe("PlanPane — plan display", () => {
  it("renders the plan summary and explanation", () => {
    render(<PlanPane plan={LOW_PLAN} mode="previewing" onApprove={() => {}} error={null} />);
    expect(screen.getByText("Read the system state")).toBeInTheDocument();
    expect(screen.getByText("Inspects the current deployment.")).toBeInTheDocument();
  });

  it("renders all step action names", () => {
    render(<PlanPane plan={HIGH_PLAN} mode="previewing" onApprove={() => {}} error={null} />);
    expect(screen.getByText("GetSystemState")).toBeInTheDocument();
    expect(screen.getByText("InstallPackages")).toBeInTheDocument();
  });

  it("renders the system context line", () => {
    render(<PlanPane plan={LOW_PLAN} mode="previewing" onApprove={() => {}} error={null} />);
    expect(screen.getByText(/silverblue/)).toBeInTheDocument();
    expect(screen.getByText(/fedora\/41/)).toBeInTheDocument();
  });

  it("renders an inline error when error prop is set", () => {
    const err: ShellError = { code: "role_insufficient", message: "Admin required", systemChanged: false };
    render(<PlanPane plan={LOW_PLAN} mode="previewing" onApprove={() => {}} error={err} />);
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });
});

describe("PlanPane — approval gate", () => {
  it("does not render the gate in previewing mode", () => {
    render(<PlanPane plan={LOW_PLAN} mode="previewing" onApprove={() => {}} error={null} />);
    expect(screen.queryByRole("button", { name: /approve/i })).toBeNull();
  });

  it("LOW risk: renders a single Approve button in awaiting-approval mode", () => {
    render(<PlanPane plan={LOW_PLAN} mode="awaiting-approval" onApprove={() => {}} error={null} />);
    expect(screen.getByRole("button", { name: /approve/i })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("HIGH risk: Approve button is disabled until action name is typed", () => {
    render(<PlanPane plan={HIGH_PLAN} mode="awaiting-approval" onApprove={() => {}} error={null} />);
    const approveBtn = screen.getByRole("button", { name: /approve/i });
    expect(approveBtn).toBeDisabled();

    fireEvent.change(screen.getByRole("textbox"), { target: { value: "InstallPackages" } });
    expect(approveBtn).not.toBeDisabled();
  });

  it("HIGH risk: Approve button calls onApprove when clicked", () => {
    const onApprove = vi.fn();
    render(<PlanPane plan={HIGH_PLAN} mode="awaiting-approval" onApprove={onApprove} error={null} />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "InstallPackages" } });
    fireEvent.click(screen.getByRole("button", { name: /approve/i }));
    expect(onApprove).toHaveBeenCalledOnce();
  });
});
```

Run:
```bash
cd apps/lacs-shell && npx vitest run src/components/PlanPane.test.tsx --reporter=verbose
```
Expected: FAIL — existing PlanPane does not have these props

- [ ] **Step 2: Rewrite `PlanPane.tsx`**

```typescript
import { useState } from "react";
import type { PlanResponse, ShellError } from "../types";
import type { ShellMode } from "../shellState";
import { RiskBadge } from "./RiskBadge";
import { ErrorBlock } from "./ErrorBlock";

interface Props {
  plan: PlanResponse;
  mode: ShellMode;
  onApprove: () => void;
  error: ShellError | null;
}

function aggregateRisk(steps: PlanResponse["steps"]): "low" | "medium" | "high" {
  if (steps.some((s) => s.riskLevel === "high")) return "high";
  if (steps.some((s) => s.riskLevel === "medium")) return "medium";
  return "low";
}

function firstHighRiskName(steps: PlanResponse["steps"]): string {
  return steps.find((s) => s.riskLevel === "high")?.actionName ?? "";
}

export function PlanPane({ plan, mode, onApprove, error }: Props) {
  const [expanded, setExpanded] = useState(false);
  const [checked, setChecked] = useState(false);
  const [confirmText, setConfirmText] = useState("");

  const risk = aggregateRisk(plan.steps);
  const showGate = mode === "awaiting-approval";
  const highRiskName = firstHighRiskName(plan.steps);
  const SHOW_THRESHOLD = 4;
  const visibleSteps = expanded ? plan.steps : plan.steps.slice(0, SHOW_THRESHOLD - 1);
  const hiddenCount = plan.steps.length - visibleSteps.length;

  const approveEnabled =
    risk === "low" ||
    (risk === "medium" && checked) ||
    (risk === "high" && confirmText === highRiskName);

  return (
    <section className="pane pane-plan">
      <div className="pane-header">
        <h2>Plan</h2>
        <RiskBadge level={risk} />
      </div>

      <p className="plan-summary">{plan.summary}</p>
      <p className="plan-explanation">{plan.explanation}</p>

      {plan.steps.some((s) => s.approvalRequired) && (
        <div className="plan-reboot-banner" role="note">
          ⚠ Reboot may be required after execution
        </div>
      )}

      <ol className="plan-steps">
        {visibleSteps.map((step, i) => (
          <li key={step.actionName} className="plan-step">
            <span className="plan-step__index">{i + 1}</span>
            <code className="plan-step__name">{step.actionName}</code>
            <span className="plan-step__summary">{step.summary}</span>
            <RiskBadge level={step.riskLevel} />
            {step.approvalRequired && (
              <span className="plan-step__approval-note">↳ approval required</span>
            )}
          </li>
        ))}
      </ol>

      {hiddenCount > 0 && (
        <button type="button" className="plan-expand" onClick={() => setExpanded(true)}>
          Show {hiddenCount} more step{hiddenCount > 1 ? "s" : ""} ↓
        </button>
      )}

      <p className="plan-context">
        <code>{plan.hostName} · {plan.deployment} · {plan.toolboxCount} toolbox{plan.toolboxCount !== 1 ? "es" : ""} · {plan.flatpakCount} flatpak{plan.flatpakCount !== 1 ? "s" : ""}</code>
      </p>

      {error && <ErrorBlock error={error} />}

      {showGate && (
        <div className="approval-gate">
          <hr />
          {risk === "low" && (
            <button type="button" onClick={onApprove}>
              Approve
            </button>
          )}
          {risk === "medium" && (
            <>
              <label className="approval-gate__checkbox">
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={(e) => setChecked(e.target.checked)}
                />
                I understand this will modify system state
              </label>
              <button type="button" disabled={!checked} onClick={onApprove}>
                Approve
              </button>
            </>
          )}
          {risk === "high" && (
            <>
              <label className="approval-gate__confirm">
                Type <code>{highRiskName}</code> to confirm:
                <input
                  type="text"
                  value={confirmText}
                  onChange={(e) => setConfirmText(e.target.value)}
                />
              </label>
              <button type="button" disabled={!approveEnabled} onClick={onApprove}>
                Approve
              </button>
            </>
          )}
        </div>
      )}
    </section>
  );
}
```

- [ ] **Step 3: Run PlanPane tests**

```bash
cd apps/lacs-shell && npx vitest run src/components/PlanPane.test.tsx --reporter=verbose
```
Expected: all PASS

- [ ] **Step 4: Commit**

```bash
git add apps/lacs-shell/src/components/PlanPane.tsx apps/lacs-shell/src/components/PlanPane.test.tsx
git commit -m "feat(shell): rewrite PlanPane with step breakdown, risk badges, and approval gate"
```

---

## Task 8: Update `IntentPane`, `ExecutionPane`, `TimelinePane`

**Files:**
- Modify: `apps/lacs-shell/src/components/IntentPane.tsx`
- Modify: `apps/lacs-shell/src/components/ExecutionPane.tsx`
- Modify: `apps/lacs-shell/src/components/TimelinePane.tsx`

- [ ] **Step 1: Rewrite `IntentPane.tsx`**

```typescript
import type { FormEvent } from "react";
import type { ShellMode } from "../shellState";
import type { ShellError } from "../types";
import { ErrorBlock } from "./ErrorBlock";

interface Props {
  intent: string;
  mode: ShellMode;
  onSubmit: (intent: string) => void;
  onReset: () => void;
  error: ShellError | null;
}

export function IntentPane({ intent, mode, onSubmit, onReset, error }: Props) {
  const isIdle = mode === "idle";

  if (!isIdle) {
    // Compact read-only strip
    return (
      <section className="pane pane-intent pane-intent--compact">
        <p className="eyebrow">What should LACS do?</p>
        <p className="intent-submitted">{intent}</p>
        {mode !== "executing" && (
          <button type="button" onClick={onReset} className="intent-reset">
            Reset
          </button>
        )}
      </section>
    );
  }

  return (
    <section className="pane pane-intent">
      <h2>Intent</h2>
      {error && <ErrorBlock error={error} onRetry={() => {}} />}
      <form
        className="intent-form"
        onSubmit={(event: FormEvent<HTMLFormElement>) => {
          event.preventDefault();
          const formData = new FormData(event.currentTarget);
          onSubmit(String(formData.get("intent") ?? "").trim());
        }}
      >
        <label className="field">
          <span>What should LACS do?</span>
          <input
            name="intent"
            defaultValue={intent}
            placeholder="describe a Linux administration task — e.g. 'install vim', 'rebase to Fedora 42'"
          />
        </label>
        <button type="submit">Generate plan</button>
      </form>
    </section>
  );
}
```

- [ ] **Step 2: Rewrite `ExecutionPane.tsx`**

```typescript
import { useState } from "react";
import type { ShellMode } from "../shellState";
import type { PlanResponse, ShellError } from "../types";
import { ErrorBlock } from "./ErrorBlock";

interface Props {
  mode: ShellMode;
  plan: PlanResponse | null;
  activeJobId: string | null;
  onCancel: () => void;
  onReset: () => void;
  executionError?: ShellError;
}

export function ExecutionPane({ mode, plan, activeJobId, onCancel, onReset, executionError }: Props) {
  const [isCanceling, setIsCanceling] = useState(false);

  const handleCancel = () => {
    setIsCanceling(true);
    onCancel();
  };

  if (mode === "failed" && executionError) {
    return (
      <section className="pane pane-execution">
        <h2>Execution</h2>
        <ErrorBlock error={executionError} onReset={onReset} />
        <button type="button" onClick={onReset}>New task</button>
      </section>
    );
  }

  if (mode === "succeeded" as ShellMode || mode === "idle") {
    return null;
  }

  if (mode === "needs-reboot" || mode === "rolled-back") {
    return (
      <section className="pane pane-execution">
        <h2>
          {mode === "needs-reboot" ? "Completed" : "Rolled back"}
        </h2>
        {mode === "needs-reboot" && (
          <div className="execution-reboot-banner" role="note">
            <p>⚠ Reboot required to apply changes.</p>
            <p>Run: <code>systemctl reboot</code></p>
          </div>
        )}
        <button type="button" onClick={onReset}>New task</button>
      </section>
    );
  }

  return (
    <section className="pane pane-execution">
      <div className="pane-header">
        <h2>Executing</h2>
        {activeJobId && <code className="execution-job-id">Job: {activeJobId}</code>}
      </div>

      {plan && (
        <ol className="execution-steps">
          {plan.steps.map((step, i) => (
            <li key={step.actionName} className="execution-step">
              <span className="execution-step__icon" aria-hidden>○</span>
              <span className="execution-step__index">{i + 1}/{plan.steps.length}</span>
              <code className="execution-step__name">{step.actionName}</code>
              <span className="execution-step__summary">{step.summary}</span>
            </li>
          ))}
        </ol>
      )}

      <div className="execution-actions">
        <button
          type="button"
          onClick={handleCancel}
          disabled={isCanceling}
        >
          {isCanceling ? "Canceling..." : "Cancel"}
        </button>
      </div>
    </section>
  );
}
```

> **Note on step icons:** The `○` / `◐` / `✓` / `✗` icons require live status from the daemon (step-by-step progress events). With the current stub implementation (no daemon IPC), all steps show `○`. Replace with live icons when daemon events are wired in the follow-on IPC plan.

- [ ] **Step 3: Rewrite `TimelinePane.tsx`**

```typescript
import { useEffect, useRef } from "react";
import type { TimelineEntry, TimelineEntryKind } from "../shellState";

interface Props {
  entries: TimelineEntry[];
}

const KIND_COLORS: Record<TimelineEntryKind, string> = {
  system:  "#9db0ff",
  user:    "#8ca2ff",
  success: "#4ade80",
  warning: "#fb923c",
  error:   "#f87171",
};

export function TimelinePane({ entries }: Props) {
  const bottomRef = useRef<HTMLLIElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [entries]);

  return (
    <section className="pane pane-timeline">
      <h2>Timeline</h2>
      <ol className="timeline" aria-live="polite" aria-label="Event log">
        {entries.length === 0 && <li className="timeline-empty">No events yet</li>}
        {entries.map((entry, i) => (
          <li
            key={entry.id}
            className="timeline-entry"
            ref={i === entries.length - 1 ? bottomRef : null}
          >
            <span
              className="timeline-entry__dot"
              style={{ color: KIND_COLORS[entry.kind] }}
              aria-hidden
            >
              ●
            </span>
            <time className="timeline-entry__timestamp">{entry.timestamp}</time>
            <span className="timeline-entry__text">{entry.text}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}
```

- [ ] **Step 4: Verify TypeScript compiles**

```bash
cd apps/lacs-shell && npx tsc --noEmit
```
Expected: zero errors (App.tsx is updated next — ignore errors from it for now)

- [ ] **Step 5: Commit**

```bash
git add apps/lacs-shell/src/components/IntentPane.tsx \
        apps/lacs-shell/src/components/ExecutionPane.tsx \
        apps/lacs-shell/src/components/TimelinePane.tsx
git commit -m "feat(shell): update IntentPane, ExecutionPane, TimelinePane"
```

---

## Task 9: Update `App.tsx` and `App.test.tsx`

**Files:**
- Modify: `apps/lacs-shell/src/App.tsx`
- Modify: `apps/lacs-shell/src/App.test.tsx`

- [ ] **Step 1: Write failing App tests**

Replace `apps/lacs-shell/src/App.test.tsx`:

```typescript
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import App from "./App";
import * as bridge from "./daemonBridge";
import type { PlanResponse } from "./types";

vi.mock("./daemonBridge", () => ({
  requestPlan: vi.fn(),
  requestApproval: vi.fn().mockResolvedValue(undefined),
  cancelJob: vi.fn().mockResolvedValue(undefined),
  getBrainConfig: vi.fn().mockResolvedValue({
    provider: "ollama",
    model: "mistral:7b",
    fallback: false,
  }),
  subscribeDaemonEvents: vi.fn().mockResolvedValue(() => undefined),
}));

const mockedRequestPlan = vi.mocked(bridge.requestPlan);

const READ_ONLY_PLAN: PlanResponse = {
  summary: "Inspect system state",
  explanation: "Reads the current deployment.",
  approvalRequired: false,
  steps: [
    { actionName: "GetSystemState", summary: "Read state", riskLevel: "low", approvalRequired: false },
  ],
  hostName: "silverblue", deployment: "fedora/41", toolboxCount: 1, flatpakCount: 2,
};

const MUTATING_PLAN: PlanResponse = {
  summary: "Install vim",
  explanation: "Layers vim via rpm-ostree.",
  approvalRequired: true,
  steps: [
    { actionName: "InstallPackages", summary: "Layer vim", riskLevel: "high", approvalRequired: true },
  ],
  hostName: "silverblue", deployment: "fedora/41", toolboxCount: 0, flatpakCount: 0,
};

describe("App", () => {
  it("renders the shell and shows Ready status", () => {
    render(<App />);
    expect(screen.getByRole("status")).toHaveTextContent("Ready");
  });

  it("shows 'Review plan' status for read-only plan (no approval required)", async () => {
    mockedRequestPlan.mockResolvedValueOnce(READ_ONLY_PLAN);
    render(<App />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "show state" } });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Review plan");
    });
  });

  it("transitions to 'Awaiting your approval' for mutating plans", async () => {
    mockedRequestPlan.mockResolvedValueOnce(MUTATING_PLAN);
    render(<App />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "install vim" } });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Awaiting your approval");
    });
  });

  it("stays on Ready and shows error when requestPlan rejects", async () => {
    mockedRequestPlan.mockRejectedValueOnce({
      code: "llm_http_error",
      message: "HTTP 500",
      systemChanged: false,
    });
    render(<App />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "install vim" } });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Ready");
    });
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });

  it("sets grid data-mode attribute to the current mode", async () => {
    mockedRequestPlan.mockResolvedValueOnce(MUTATING_PLAN);
    render(<App />);
    // idle → grid has data-mode="idle"
    expect(document.querySelector(".grid")?.getAttribute("data-mode")).toBe("idle");

    fireEvent.change(screen.getByRole("textbox"), { target: { value: "install vim" } });
    fireEvent.click(screen.getByRole("button", { name: /generate plan/i }));

    await waitFor(() => {
      expect(document.querySelector(".grid")?.getAttribute("data-mode")).toBe("awaiting-approval");
    });
  });
});
```

Run:
```bash
cd apps/lacs-shell && npx vitest run src/App.test.tsx --reporter=verbose
```
Expected: FAILs — status badge text "Ready" not found, `data-mode` not set, etc.

- [ ] **Step 2: Rewrite `App.tsx`**

```typescript
import { useEffect, useReducer, useCallback } from "react";
import { IntentPane } from "./components/IntentPane";
import { ExecutionPane } from "./components/ExecutionPane";
import { PlanPane } from "./components/PlanPane";
import { TimelinePane } from "./components/TimelinePane";
import {
  getBrainConfig,
  requestPlan,
  requestApproval,
  cancelJob,
  subscribeDaemonEvents,
} from "./daemonBridge";
import {
  initialShellState,
  shellReducer,
} from "./shellState";
import type { BrainConfigResponse, ShellError } from "./types";
import { useState } from "react";

const STATUS_LABELS: Record<string, string> = {
  idle:                "Ready",
  planning:            "Planning...",
  previewing:          "Review plan",
  "awaiting-approval": "Awaiting your approval",
  executing:           "Executing...",
  "needs-reboot":      "Done — reboot required",
  failed:              "Failed",
  "rolled-back":       "Rolled back",
};

export default function App() {
  const [state, dispatch] = useReducer(shellReducer, initialShellState);
  const [brainConfig, setBrainConfig] = useState<BrainConfigResponse | null>(null);

  // Load brain config once on mount
  useEffect(() => {
    getBrainConfig()
      .then((cfg) => {
        setBrainConfig(cfg);
        if (cfg.fallback) {
          dispatch({
            type: "timeline_event",
            text: `Brain config fallback: using ${cfg.provider}/${cfg.model}`,
            kind: "warning",
          });
        }
      })
      .catch(() => {
        // Non-fatal — UI degrades gracefully without the provider label
      });
  }, []);

  // Subscribe to daemon-pushed timeline and outcome events
  useEffect(() => {
    let unsubscribeFn: (() => void) | null = null;
    let cancelled = false;

    subscribeDaemonEvents(
      (payload) => {
        if (!cancelled) {
          dispatch({ type: "timeline_event", text: payload.text, kind: "system" });
        }
      },
      (outcome) => {
        if (!cancelled) {
          dispatch({ type: "job_completed", outcome });
        }
      },
    )
      .then((unsub) => {
        if (cancelled) unsub();
        else unsubscribeFn = unsub;
      })
      .catch(() => {
        if (!cancelled) {
          dispatch({ type: "daemon_status_changed", status: "unreachable" });
        }
      });

    return () => {
      cancelled = true;
      unsubscribeFn?.();
    };
  }, []);

  const handleIntent = useCallback(async (intent: string) => {
    if (!intent) return;
    dispatch({ type: "intent_submitted", intent });

    try {
      const plan = await requestPlan(intent);
      dispatch({ type: "daemon_status_changed", status: "connected" });
      dispatch({ type: "plan_ready", plan });
      if (plan.approvalRequired) {
        dispatch({ type: "request_approval" });
      } else {
        dispatch({ type: "timeline_event", text: `Read-only plan completed: ${intent}`, kind: "success" });
        dispatch({ type: "job_completed", outcome: "succeeded" });
      }
    } catch (err) {
      dispatch({ type: "daemon_status_changed", status: "unreachable" });
      const shellError: ShellError =
        err && typeof err === "object" && "code" in err
          ? (err as ShellError)
          : { code: "unknown", message: String(err), systemChanged: false };
      dispatch({ type: "plan_errored", error: shellError });
    }
  }, []);

  const handleApprove = useCallback(async () => {
    dispatch({ type: "approval_granted" });
    try {
      await requestApproval("");  // TODO(daemon-ipc): pass real requestHash from plan
      dispatch({ type: "daemon_status_changed", status: "connected" });
    } catch (err) {
      const shellError: ShellError =
        err && typeof err === "object" && "code" in err
          ? (err as ShellError)
          : { code: "unknown", message: String(err), systemChanged: false };
      dispatch({ type: "policy_errored", error: shellError });
    }
  }, []);

  const handleCancel = useCallback(async () => {
    dispatch({ type: "cancel_requested" });
    if (state.activeJobId) {
      try {
        await cancelJob(state.activeJobId);
      } catch {
        // Cancel failed — daemon will resolve the job eventually
      }
    }
  }, [state.activeJobId]);

  const handleReset = useCallback(() => {
    dispatch({ type: "reset" });
  }, []);

  const plan = state.mode === "previewing" || state.mode === "awaiting-approval"
    ? state.plan
    : state.mode === "executing" || state.mode === "needs-reboot"
      || state.mode === "rolled-back" || state.mode === "failed"
        ? state.plan
        : null;

  const idleError = state.mode === "idle" ? state.error : null;
  const planError = (state.mode === "previewing" || state.mode === "awaiting-approval")
    ? state.planError
    : null;

  const daemonLabel =
    state.daemonStatus === "connected" ? "daemon: connected" :
    state.daemonStatus === "unreachable" ? "daemon: unreachable" :
    "daemon: unknown";

  return (
    <main className="app-shell">
      <header className="app-header">
        <div>
          <p className="eyebrow">LACS</p>
          <h1>Linux Agent Control Standard</h1>
        </div>
        <div className="app-header__right">
          <div className="status-badge" role="status">
            {STATUS_LABELS[state.mode] ?? state.mode}
          </div>
          {brainConfig && (
            <p className="provider-label">
              {brainConfig.fallback && <span className="provider-label__fallback">⚠ </span>}
              via {brainConfig.provider}/{brainConfig.model}
            </p>
          )}
          <p className={`daemon-indicator daemon-indicator--${state.daemonStatus}`}>
            ● {daemonLabel}
          </p>
          {state.mode !== "executing" && (
            <button type="button" className="reset-btn" onClick={handleReset}>
              Reset
            </button>
          )}
        </div>
      </header>

      <section className="grid" data-mode={state.mode}>
        <IntentPane
          intent={state.intent}
          mode={state.mode}
          onSubmit={handleIntent}
          onReset={handleReset}
          error={idleError}
        />
        {(state.mode === "previewing" || state.mode === "awaiting-approval") && plan && (
          <PlanPane
            plan={plan}
            mode={state.mode}
            onApprove={handleApprove}
            error={planError}
          />
        )}
        {(state.mode === "executing" || state.mode === "needs-reboot"
          || state.mode === "rolled-back" || state.mode === "failed") && (
          <ExecutionPane
            mode={state.mode}
            plan={plan}
            activeJobId={state.activeJobId}
            onCancel={handleCancel}
            onReset={handleReset}
          />
        )}
        <TimelinePane entries={state.timeline} />
      </section>
    </main>
  );
}
```

- [ ] **Step 3: Run App tests**

```bash
cd apps/lacs-shell && npx vitest run src/App.test.tsx --reporter=verbose
```
Expected: all PASS

- [ ] **Step 4: Run full TypeScript suite**

```bash
cd apps/lacs-shell && npx vitest run --reporter=verbose
```
Expected: all PASS across all test files

- [ ] **Step 5: Commit**

```bash
git add apps/lacs-shell/src/App.tsx apps/lacs-shell/src/App.test.tsx
git commit -m "feat(shell): rewrite App.tsx — state-driven layout, brain config, plan_errored dispatch"
```

---

## Task 10: Update `styles.css` — grid templates and semantic colours

**Files:**
- Modify: `apps/lacs-shell/src/styles.css`

- [ ] **Step 1: Replace `styles.css`**

```css
:root {
  color-scheme: dark;
  font-family:
    "Inter",
    "SF Pro Display",
    "Segoe UI",
    system-ui,
    sans-serif;
  background:
    radial-gradient(circle at top left, rgba(61, 88, 255, 0.26), transparent 36%),
    linear-gradient(180deg, #0b1020, #070a12);
  color: #f4f7fb;
}

* { box-sizing: border-box; }

body {
  margin: 0;
  min-height: 100vh;
  background: transparent;
}

button, input { font: inherit; }

/* ---------------------------------------------------------------------------
   App shell
--------------------------------------------------------------------------- */

.app-shell { min-height: 100vh; padding: 32px; }

.app-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 24px;
}

.app-header__right {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 6px;
}

.eyebrow {
  margin: 0 0 8px;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  font-size: 0.78rem;
  color: #9db0ff;
}

.app-header h1 {
  margin: 0;
  font-size: clamp(1.6rem, 4vw, 2.8rem);
  line-height: 1;
}

.status-badge {
  padding: 8px 16px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.12);
}

.provider-label {
  margin: 0;
  font-size: 0.75rem;
  font-family: monospace;
  color: #9db0ff;
}

.provider-label__fallback { color: #fb923c; }

.daemon-indicator {
  margin: 0;
  font-size: 0.75rem;
  font-family: monospace;
}
.daemon-indicator--connected  { color: #4ade80; }
.daemon-indicator--unreachable { color: #f87171; }
.daemon-indicator--unknown    { color: #9db0ff; }

.reset-btn {
  border: 1px solid rgba(148, 163, 184, 0.3);
  border-radius: 999px;
  padding: 6px 14px;
  background: transparent;
  color: #c2cbdf;
  cursor: pointer;
}

/* ---------------------------------------------------------------------------
   Grid — state-driven layout
--------------------------------------------------------------------------- */

.grid {
  display: grid;
  gap: 16px;
  /* default: idle / planning */
  grid-template-areas:
    "intent  timeline"
    "intent  timeline";
  grid-template-columns: 1fr 280px;
  grid-template-rows: 1fr;
  min-height: 60vh;
}

.grid[data-mode="idle"],
.grid[data-mode="planning"] {
  grid-template-areas:
    "intent  timeline"
    "intent  timeline";
  grid-template-columns: 1fr 280px;
}

.grid[data-mode="previewing"],
.grid[data-mode="awaiting-approval"] {
  grid-template-areas:
    "plan     plan    "
    "intent   timeline";
  grid-template-columns: 1fr 280px;
  grid-template-rows: auto 180px;
}

.grid[data-mode="executing"],
.grid[data-mode="needs-reboot"],
.grid[data-mode="rolled-back"] {
  grid-template-areas:
    "execution  timeline"
    "intent     timeline";
  grid-template-columns: 1fr 280px;
  grid-template-rows: 1fr auto;
}

.grid[data-mode="failed"] {
  grid-template-areas:
    "execution  execution"
    "intent     timeline";
  grid-template-columns: 1fr 280px;
  grid-template-rows: 1fr auto;
}

/* ---------------------------------------------------------------------------
   Pane base
--------------------------------------------------------------------------- */

.pane {
  padding: 20px;
  border-radius: 20px;
  background: rgba(10, 16, 29, 0.82);
  border: 1px solid rgba(148, 163, 184, 0.16);
  backdrop-filter: blur(12px);
  overflow-y: auto;
}

.pane h2 { margin-top: 0; }
.pane-header { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 12px; }

.pane-intent    { grid-area: intent; }
.pane-plan      { grid-area: plan; }
.pane-execution { grid-area: execution; }
.pane-timeline  { grid-area: timeline; }

.pane-intent--compact {
  display: flex;
  align-items: center;
  gap: 16px;
  min-height: 0;
  padding: 12px 20px;
}

/* ---------------------------------------------------------------------------
   IntentPane
--------------------------------------------------------------------------- */

.intent-form { display: grid; gap: 12px; }
.field { display: grid; gap: 6px; }

input {
  border-radius: 12px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  padding: 12px 14px;
  background: rgba(255, 255, 255, 0.06);
  color: inherit;
}

button {
  border: 0;
  border-radius: 999px;
  padding: 10px 16px;
  background: #8ca2ff;
  color: #08101d;
  font-weight: 600;
  cursor: pointer;
}

button:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.intent-submitted { margin: 0; flex: 1; font-style: italic; color: #c2cbdf; }
.intent-reset { background: transparent; border: 1px solid rgba(148,163,184,0.3); color: #c2cbdf; }

/* ---------------------------------------------------------------------------
   PlanPane
--------------------------------------------------------------------------- */

.plan-summary { font-size: 1.1rem; font-weight: 600; margin: 0 0 6px; }
.plan-explanation { color: #c2cbdf; margin: 0 0 12px; }

.plan-reboot-banner {
  background: rgba(251, 146, 60, 0.12);
  border: 1px solid rgba(251, 146, 60, 0.4);
  border-radius: 8px;
  padding: 8px 12px;
  color: #fb923c;
  margin-bottom: 12px;
}

.plan-steps { list-style: none; padding: 0; margin: 0 0 12px; display: grid; gap: 6px; }
.plan-step {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid rgba(148, 163, 184, 0.12);
  background: rgba(255,255,255,0.03);
}
.plan-step__index { color: #9db0ff; font-size: 0.8rem; min-width: 16px; }
.plan-step__name { font-family: monospace; font-size: 0.85rem; }
.plan-step__summary { flex: 1; color: #c2cbdf; font-size: 0.9rem; }
.plan-step__approval-note { font-size: 0.78rem; color: #9db0ff; }

.plan-expand { background: transparent; border: none; color: #9db0ff; cursor: pointer; padding: 4px 0; font-size: 0.85rem; }
.plan-context { font-family: monospace; font-size: 0.78rem; color: #9db0ff; margin: 8px 0 0; }

/* Risk badge */
.risk-badge {
  font-size: 0.72rem;
  font-weight: 700;
  padding: 2px 8px;
  border-radius: 999px;
  white-space: nowrap;
}

/* Approval gate */
.approval-gate { margin-top: 16px; display: grid; gap: 12px; }
.approval-gate hr { border: none; border-top: 1px solid rgba(148,163,184,0.16); margin: 0; }
.approval-gate__checkbox { display: flex; align-items: center; gap: 8px; cursor: pointer; }
.approval-gate__confirm { display: grid; gap: 6px; }
.approval-gate__confirm code { color: #fb923c; }

/* ---------------------------------------------------------------------------
   ExecutionPane
--------------------------------------------------------------------------- */

.execution-job-id { font-size: 0.78rem; color: #9db0ff; }
.execution-steps { list-style: none; padding: 0; margin: 0 0 16px; display: grid; gap: 6px; }
.execution-step {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: 8px;
  background: rgba(255,255,255,0.03);
}
.execution-step__icon { color: #9db0ff; }
.execution-step__index { color: #9db0ff; font-size: 0.78rem; }
.execution-step__name { font-family: monospace; font-size: 0.85rem; }
.execution-step__summary { flex: 1; color: #c2cbdf; font-size: 0.9rem; }
.execution-actions { display: flex; justify-content: flex-end; }
.execution-reboot-banner {
  background: rgba(251, 146, 60, 0.12);
  border: 1px solid rgba(251, 146, 60, 0.4);
  border-radius: 8px;
  padding: 12px 16px;
  color: #fb923c;
  margin-bottom: 16px;
}
.execution-reboot-banner code { font-family: monospace; }

/* ---------------------------------------------------------------------------
   ErrorBlock
--------------------------------------------------------------------------- */

.error-block {
  border: 1px solid rgba(248, 113, 113, 0.35);
  background: rgba(248, 113, 113, 0.06);
  border-radius: 12px;
  padding: 16px;
  display: grid;
  gap: 8px;
  margin-bottom: 12px;
}
.error-block__header { display: flex; align-items: center; gap: 8px; }
.error-block__title { color: #f87171; }
.error-block__message { margin: 0; color: #c2cbdf; font-size: 0.9rem; }
.error-block__state { margin: 0; font-size: 0.85rem; color: #9db0ff; }
.error-block__state--warning { color: #fb923c; }
.error-block__actions { display: flex; gap: 8px; }

/* ---------------------------------------------------------------------------
   Timeline
--------------------------------------------------------------------------- */

.pane-timeline { display: flex; flex-direction: column; }

.timeline {
  list-style: none;
  padding: 0;
  margin: 0;
  flex: 1;
  overflow-y: auto;
  display: grid;
  gap: 6px;
  align-content: start;
}

.timeline-empty { color: #9db0ff; font-size: 0.85rem; }

.timeline-entry {
  display: grid;
  grid-template-columns: 16px auto 1fr;
  gap: 6px;
  align-items: baseline;
  font-size: 0.82rem;
}
.timeline-entry__dot { font-size: 0.6rem; }
.timeline-entry__timestamp { font-family: monospace; color: #9db0ff; white-space: nowrap; }
.timeline-entry__text { color: #dfe6ff; }

/* ---------------------------------------------------------------------------
   Responsive
--------------------------------------------------------------------------- */

@media (max-width: 860px) {
  .grid,
  .grid[data-mode="idle"],
  .grid[data-mode="planning"],
  .grid[data-mode="previewing"],
  .grid[data-mode="awaiting-approval"],
  .grid[data-mode="executing"],
  .grid[data-mode="needs-reboot"],
  .grid[data-mode="rolled-back"],
  .grid[data-mode="failed"] {
    grid-template-areas: none;
    grid-template-columns: 1fr;
    grid-template-rows: auto;
  }
  .pane-intent, .pane-plan, .pane-execution, .pane-timeline {
    grid-area: unset;
  }
  .app-header { flex-direction: column; }
}
```

- [ ] **Step 2: Run full test suite to confirm nothing broke**

```bash
cd apps/lacs-shell && npx vitest run --reporter=verbose
```
Expected: all PASS

- [ ] **Step 3: Commit**

```bash
git add apps/lacs-shell/src/styles.css
git commit -m "feat(shell): state-driven grid layout and semantic colour system in styles.css"
```

---

## Task 11: Self-review pass — run all checks

- [ ] **Step 1: TypeScript compile**

```bash
cd apps/lacs-shell && npx tsc --noEmit 2>&1 | head -30
```
Expected: zero errors

- [ ] **Step 2: Full vitest suite**

```bash
cd apps/lacs-shell && npx vitest run --reporter=verbose 2>&1 | tail -20
```
Expected: all PASS, zero skipped

- [ ] **Step 3: Tauri Rust tests**

```bash
cd apps/lacs-shell/src-tauri && cargo test -- --nocapture 2>&1 | tail -10
```
Expected: `test result: ok. N passed; 0 failed`

- [ ] **Step 4: lacs-brain tests**

```bash
cargo test -p lacs-brain -- --nocapture 2>&1 | tail -5
```
Expected: `test result: ok. N passed; 0 failed`

- [ ] **Step 5: Commit if clean**

```bash
git add -A
git status  # confirm only expected files changed
git commit -m "chore(shell): self-review pass — all checks green"
```

---

## Self-review against spec

Checked every section of `docs/superpowers/specs/2026-04-10-ux-error-flows-design.md`:

| Spec section | Task(s) |
|---|---|
| 3. State-driven layout — grid templates | Tasks 9, 10 |
| 4.1 Retire ShellPreview | Task 2 |
| 4.2 TimelineEntry timestamp + kind | Tasks 2, 3 |
| 4.3 ShellAction additions | Task 2 |
| 4.4 ShellErrorCode + ShellError | Tasks 1, 4 |
| 5. PlanPane — full step breakdown | Task 7 |
| 5.4 System context fields | Task 4 |
| 6. Approval gate — risk-scaled friction | Task 7 |
| 7.0 Error dispatch model | Tasks 2, 9 |
| 7.2 Five-category error messages | Tasks 6, 8 |
| 8. ExecutionPane — step list, cancel | Task 8 |
| 8.2 Cancel as local UI state | Task 8 |
| 9. Timeline — auto-scroll, color dots | Task 8 |
| 10.2 Human-readable status badge | Task 9 |
| 10.3 Daemon connection indicator | Task 9 |
| 10.4 LLM provider indicator + fallback | Tasks 3, 4, 9 |
| 10.5 Reset button visibility | Tasks 8, 9 |
| 11. New Tauri commands | Task 4 |
| 12. IPC — Cancel in DaemonRequest | Deferred to daemon IPC plan |

**One noted gap:** `DaemonRequest::Cancel` (spec §12) is wired in the daemon-side IPC plan, not here. The shell's `cancelJob` bridge function is present; the daemon type is out of scope.

**No placeholders found in plan steps.**

**Type consistency verified:** `PlanResponse` and `PlanStepResponse` defined in Task 1 and used identically in Tasks 4, 7, 8, 9. `ShellError` defined in Task 1 and used identically in Tasks 4, 6, 8, 9. `TimelineEntryKind` defined in Task 2 and used identically in Tasks 8, 9.
