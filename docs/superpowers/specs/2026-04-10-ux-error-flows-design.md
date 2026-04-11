# LACS Shell UX — Error Flows, Approval Gate, and State-Driven Layout

**Date:** 2026-04-10
**Status:** Approved — ready for implementation planning

---

## 1. Scope

This spec covers the complete UX redesign of `lacs-shell` for its three target users:
DevOps engineers, sysadmins, and developers. It addresses:

- State-driven layout (the grid adapts to `ShellMode`)
- PlanPane enrichment (full step breakdown replaces the synthetic summary string)
- Risk-scaled approval gate
- Five-category error taxonomy with inline error blocks
- ExecutionPane with step-level progress and cancel
- Timeline as a typed, timestamped audit log
- Global chrome (daemon indicator, human-readable status, LLM provider label)
- All data model and Tauri command changes required

Out of scope for this spec (tracked separately):
- Daemon IPC dispatch loop (`connection_handler` wiring)
- Shell→Daemon IPC client (`DaemonIpcClient` replacing `DemoStateClient`)
- Streaming command output (v2)
- `Plan::new` → `Result`, `ActionName` newtype, `CuratedState` private fields (separate engineering issues)

---

## 2. Visual style

Dashboard style, expert density. Color used only for semantic meaning (risk levels, entry kinds, connection status) — never decoration. Monospace for all technical values: action names, hostnames, deployment strings, hashes, job IDs, transaction IDs, commands. No animations beyond meaningful state transitions (spinner for running steps, smooth timeline scroll).

The existing dark glassmorphic aesthetic (Inter, radial gradient, backdrop-filter panes) is kept. The problem is hollow components, not the visual foundation.

---

## 3. State-driven layout

The `<section className="grid">` element receives a `data-mode` attribute set to `state.mode`. All layout switching is CSS-only via `grid-template-areas` — no JavaScript layout logic.

### 3.1 Grid templates by mode

**`idle` / `planning`:**
```
"intent  timeline"
"intent  timeline"
```
IntentPane expands to full form. Timeline is right column.

**`previewing` / `awaiting-approval`:**
```
"plan    plan    "
"intent  timeline"
```
PlanPane spans full width. IntentPane collapses to a compact read-only strip showing the submitted intent. Timeline is bottom-right.

**`executing` / `needs-reboot`:**
```
"execution  timeline"
"intent     timeline"
```
ExecutionPane takes the main column. IntentPane is a compact strip below. Timeline spans both rows on the right.

**`failed`** (execution failure — categories 4 and 5 only):
```
"error   error   "
"intent  timeline"
```
Full-width error block. `FailedState` carries `error: ShellError`. Planning and policy errors (categories 1–3) never transition to `failed` — see Section 7.0.

**`rolled-back`** (rollback completed — always from execution):
```
"execution  timeline"
"intent     timeline"
```
Same template as `executing`. ExecutionPane shows a rollback summary ("N steps reversed") rather than an error block. Rolled-back is a recovery outcome, not an error.

### 3.2 IntentPane compact strip

When mode is not `idle`, IntentPane renders as a narrow read-only strip:

```
What should LACS do?
install vim                                              [Reset]
```

The form is unmounted. The Reset button dispatches `reset` on `shellReducer`. Reset is hidden during `executing` — Cancel in ExecutionPane is the correct escape during a live job.

### 3.3 PlanPane lifecycle

PlanPane is mounted only during `previewing` and `awaiting-approval`. It is unmounted (not hidden) in all other modes. This prevents stale plan data from being visible after a reset.

---

## 4. Data model changes

### 4.1 Retire `ShellPreview`

`ShellPreview { summary: string }` is a placeholder that discards all plan data. It is replaced everywhere by `PlanResponse` from `commands.rs`.

`ShellState` changes:

```typescript
// Before
type PreviewingState = Base & { mode: "previewing"; preview: ShellPreview; ... }

// After
type PreviewingState = Base & { mode: "previewing"; plan: PlanResponse; ... }
```

All variants that carried `preview: ShellPreview | null` now carry `plan: PlanResponse | null`. `ShellPreview` is deleted from `shellState.ts`.

### 4.2 `TimelineEntry` gains `timestamp` and `kind`

```typescript
export type TimelineEntryKind =
  | "system"    // state transitions, daemon events
  | "user"      // explicit user actions
  | "success"   // completions, step successes
  | "warning"   // reboot required, rollbacks
  | "error";    // failures, policy denials

export interface TimelineEntry {
  id: string;
  timestamp: string;      // HH:MM:SS, wall-clock, generated in appendTimeline
  kind: TimelineEntryKind;
  text: string;
}
```

`appendTimeline` is updated:
```typescript
function appendTimeline<S extends ShellState>(
  state: S,
  text: string,
  kind: TimelineEntryKind,
): S { ... }
```

`kind` is derived from the action type in `shellReducer` for all built-in transitions. The `timeline_event` `ShellAction` gains a `kind` field so daemon-pushed events are typed at the callsite.

### 4.3 `ShellAction` changes

```typescript
| { type: "timeline_event"; text: string; kind: TimelineEntryKind }
| { type: "cancel_requested" }
| { type: "daemon_status_changed"; status: DaemonStatus }
| { type: "plan_errored"; error: ShellError }    // categories 1 & 2 — keeps mode idle
| { type: "policy_errored"; error: ShellError }  // category 3 — keeps mode previewing/awaiting
```

`DaemonStatus` is `"unknown" | "connected" | "unreachable"`.

`ShellOutcome` gains `"canceled"`:
```typescript
export type ShellOutcome =
  | "succeeded"
  | "needs_reboot"
  | "failed"       // execution failure — transitions to failed mode
  | "rolled_back"  // rollback completed
  | "canceled";    // job was canceled by the user
```

`IdleState` gains `error: ShellError | null` (shown inline in IntentPane after a planning failure, cleared on the next `intent_submitted`). `PreviewingState` and `ApprovingState` gain `planError: ShellError | null` (shown inline in PlanPane after a policy error, cleared when user dismisses or resets).

### 4.4 `ShellErrorCode` enum

Raw error strings from Rust never reach the user. The Tauri commands layer maps errors to a typed code before serialization:

```typescript
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
  message: string;       // specific detail (e.g. "HTTP 429", action name, role name)
  systemChanged: boolean;
}
```

The Tauri `plan_intent` and `approve_preview` commands return `Result<_, ShellError>` (serialized as `{ code, message, systemChanged }`). The frontend pattern-matches on `code` to render the correct error block.

---

## 5. PlanPane

### 5.1 Layout

```
┌─ Plan ─────────────────────────────────────── ● HIGH RISK ─┐
│                                                              │
│  Install vim on this machine                                 │
│  rpm-ostree will layer vim onto the current deployment.      │
│  A reboot will be required to apply the change.             │
│                                                              │
│  ⚠ Reboot required after execution          [amber banner]  │
│  ⚠ Warning: [text]                          [if present]    │
│                                                              │
│  Steps                                                       │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  1  GetSystemState    Read current deployment  ● LOW │   │
│  └──────────────────────────────────────────────────────┘   │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  2  InstallPackages   Layer via rpm-ostree    ● HIGH │   │
│  │     ↳ approval required                              │   │
│  └──────────────────────────────────────────────────────┘   │
│  [ Show 2 more steps ↓ ]              (if > 4 steps)        │
│                                                              │
│  silverblue · fedora/41 · 1 toolbox · 1 flatpak             │
│                                                              │
│  ──────────────────────────────────────────────────────────  │
│  [Approval gate — rendered only in awaiting-approval mode]   │
└──────────────────────────────────────────────────────────────┘
```

### 5.2 Risk badges

- `low` → background `#166534`, text `#4ade80`
- `medium` → background `#7c2d12`, text `#fb923c`
- `high` → background `#7f1d1d`, text `#f87171`

Aggregate risk badge (top-right of pane header) = `max(step.risk_level)` over all steps.

### 5.3 Progressive disclosure

Plans with ≤ 4 steps: all steps rendered expanded. Plans with > 4 steps: first 3 shown, remainder behind "Show N more steps ↓" toggle. Warnings and reboot banner are never collapsed.

### 5.4 System context line

Monospace line at the bottom of the step list: `hostname · deployment · N toolboxes · N flatpaks`.

`PlanResponse` in `commands.rs` is extended with system context fields populated from `StateClient::curated_state()` at plan time (the Tauri command already holds the `StateClient`):

```rust
pub struct PlanResponse {
    // existing fields ...
    pub host_name: String,
    pub deployment: String,
    pub toolbox_count: usize,
    pub flatpak_count: usize,
}
```

No separate Tauri command needed.

---

## 6. Approval gate

The approval gate renders inside PlanPane, below the step list, only when `mode === "awaiting-approval"`. Spatial adjacency to the steps is intentional — the user approves what they just reviewed, not a separate interface.

### 6.1 Risk-scaled friction

Aggregate risk level drives the gate variant. When the aggregate risk is HIGH, the type-to-confirm field uses the `action_name` of the first HIGH-risk step (lowest index in `steps[]`). If multiple steps are HIGH, the first one is used — it is the most prominent and immediately visible in the step list.

**LOW:**
```
[ Approve ]
```

**MEDIUM:**
```
[ ☐ I understand this will modify system state ]
[ Approve ]   ← disabled until checked
```

**HIGH:**
```
Type "InstallPackages" to confirm:
[ _________________ ]
[ Approve ]   ← disabled until input matches action_name exactly
```

### 6.2 Request hash

Below the gate, in small monospace, always:
```
Request: a3f9c2d1...
```
For auditability. Not interactive.

### 6.3 Approve action

On click: dispatches `approval_granted`, calls `daemonBridge.requestApproval(requestHash)`. The bridge passes `requestHash` to the `approve_preview` Tauri command, which forwards it to the daemon over the Unix socket (once the IPC client is wired).

---

## 7. Five-category error taxonomy

### 7.0 Error dispatch model

Errors are dispatched differently based on whether they are recoverable:

| Categories | Dispatch | Mode change | Where displayed |
|---|---|---|---|
| 1 (pre-flight), 2 (planning) | `plan_errored` | None — stays `idle` | Inline in IntentPane |
| 3 (policy) | `policy_errored` | None — stays `previewing` / `awaiting-approval` | Inline in PlanPane |
| 4–5 (execution) | `job_completed: "failed"` | → `failed` | Full-width `error` grid area |

Categories 1–3 are recoverable: the user edits their intent or retries without losing context. Categories 4–5 are terminal: something ran on the system and something went wrong. The distinction drives `FailedState` and the grid layout.

### 7.1 Error block anatomy

Every error renders as a structured block in the pane that owns the failure. Three-line format, always:

```
┌─────────────────────────────────────────────────────────────┐
│ ⛔  [Title]                              [CATEGORY BADGE]   │
│                                                              │
│ [Specific cause — never a raw error string]                 │
│                                                              │
│ [System state line]                                         │
│                                                              │
│                                    [ Recovery CTA ]         │
└─────────────────────────────────────────────────────────────┘
```

The timeline records the event as a timestamped, `kind: "error"` audit entry — no buttons, no recovery text, just the fact.

### 7.2 Category definitions

**Category 1 — Pre-flight** (rendered in IntentPane)

| `ShellErrorCode` | Title | System state line | CTA |
|---|---|---|---|
| `daemon_not_running` | Cannot reach the LACS daemon | Nothing has changed. | `Retry connection` |
| `daemon_permission_denied` | Permission denied on daemon socket | Nothing has changed. | (shows `usermod` command) |

Daemon-not-running message:
> The socket at `unix:///run/lacs/daemon.sock` is not available.
> Start it with: `sudo systemctl start lacs-daemon`

**Category 2 — Planning** (rendered in IntentPane)

| `ShellErrorCode` | Title | System state line | CTA |
|---|---|---|---|
| `llm_rate_limit` | Could not generate a plan (rate limit) | Nothing has changed. | `Try again` |
| `llm_http_error` | Could not generate a plan (HTTP N) | Nothing has changed. | `Try again` |
| `llm_parse_error` | Could not parse the plan | Nothing has changed. | `Try again` |
| `safety_fence` | Action blocked by safety policy | Nothing has changed. | `What actions are supported?` |
| `intent_empty` | Intent is empty | Nothing has changed. | (inline, no CTA needed) |

Safety fence message:
> `DeleteAllUsers` is not a recognized LACS action. Only pre-approved administrative actions can be planned.

**Category 3 — Policy** (rendered in PlanPane)

| `ShellErrorCode` | Title | System state line | CTA |
|---|---|---|---|
| `role_insufficient` | Insufficient permissions | Nothing has changed. | `How to request elevated access` |
| `stale_approval` | Approval expired | Nothing has changed. | `Review plan again` |

Role-insufficient message:
> `RebaseSystem` requires the **Admin** role. Your current role is **Dev** (based on group membership).

Stale-approval message:
> The plan changed or too much time passed since the preview was generated. Review the updated plan and approve again.

**Category 4 — Execution, rollback available** (rendered in ExecutionPane)

> Step 2/3 failed — `InstallPackages` exited with code 1.
> Step 1/3 (`GetSystemState`) completed. Its changes have been rolled back.

CTA: `Confirm rollback` button. Dispatches a rollback request to the daemon and transitions to `rolled-back` mode.

**Category 5 — Execution, no rollback** (rendered in ExecutionPane)

> Step 2/3 failed — `ConfigureService` exited with code 1.
> ⚠ Step 1/3 (`InstallPackages`) made changes that cannot be automatically reversed. Review the timeline and restore manually if needed.

CTA: `View timeline` scrolls the timeline into view and highlights the relevant entries.

---

## 8. ExecutionPane

### 8.1 During execution

```
┌─ Executing ────────────── Job: job-7f3a2c · txn: a3f9c2... ─┐
│                                                               │
│  ✓  1/3  GetSystemState    Read current deployment     0.3s  │
│  ◐  2/3  InstallPackages   Layer vim via rpm-ostree     12s  │
│  ○  3/3  ConfigureService  Enable and start the service   —  │
│                                                               │
│                                              [ Cancel ]      │
└───────────────────────────────────────────────────────────────┘
```

Step icons: `○` queued · `◐` running (CSS animation) · `✓` succeeded · `✗` failed

Elapsed time: `0.3s`, `12s`, `1m 23s`. If a step's elapsed time exceeds 60 s with no update, the time label turns amber — passive "may be stuck" signal, no alarm.

Job ID and transaction ID in monospace in the pane header. Transaction ID arrives in `ResultEnvelope` from the daemon.

The step list mirrors the approved plan steps exactly. Same `action_name`, same `summary`, same order. Continuity between reviewed and executing is load-bearing for trust.

### 8.2 Cancel semantics

Cancel sends a cancellation request to the daemon (targeting the active job ID). The `ShellMode` stays `executing` — there is no `canceling` mode. Instead, ExecutionPane tracks a local `isCanceling: boolean` via `useState`. When `isCanceling` is true, the button label changes to "Canceling..." and the button is disabled. The mode transitions only when the daemon responds with `status: Canceled` in a `ResultEnvelope`, at which point `job_completed: "canceled"` is dispatched and the shell transitions to `idle` with the timeline recording the cancellation. Cancel never clears frontend state unilaterally.

### 8.3 After success

```
┌─ Completed ──────────────────────────── txn: a3f9c2 ────────┐
│  ✓  3 steps completed in 1m 13s                              │
│     GetSystemState · InstallPackages · ConfigureService      │
│                                         [ New task ]         │
└──────────────────────────────────────────────────────────────┘
```

### 8.4 After needs-reboot

```
┌─ Completed ──────────────────────────── txn: a3f9c2 ────────┐
│  ✓  2 steps completed in 38s                                 │
│  ┌─────────────────────────────────────────────────────┐    │
│  │ ⚠  Reboot required to apply changes.                │    │
│  │    Run:  systemctl reboot                           │    │
│  └─────────────────────────────────────────────────────┘    │
│                                         [ New task ]         │
└──────────────────────────────────────────────────────────────┘
```

No "Reboot now" button. Triggering a reboot from the shell would be an autonomous privileged action without a daemon-mediated approval gate. The command is shown; the human runs it.

### 8.5 "New task" button

Dispatches `reset` on `shellReducer`. Present after every terminal state: success, failed, rolled-back, canceled, needs-reboot.

### 8.6 v1 constraint: no streaming output

Streaming command output requires daemon-side event emission mid-execution. This is a significant IPC protocol addition. Step-level status (running / succeeded / failed + elapsed time) is sufficient for v1. Deferred to v2.

---

## 9. Timeline

### 9.1 Rendering

Each entry:
```
14:30:15  ·  Step 1/3 GetSystemState — completed (0.3s)
```

A colored dot (`·`) precedes each entry:
- `system` → `#9db0ff`
- `user` → `#8ca2ff`
- `success` → `#4ade80`
- `warning` → `#fb923c`
- `error` → `#f87171`

Timestamp in monospace. Text in normal weight. No buttons, no links, no CTAs — ever. The timeline is a log, not a help surface.

### 9.2 Auto-scroll

`useEffect` with a ref on the last `<li>` element. `scrollIntoView({ behavior: 'smooth', block: 'end' })` fires on every `entries` change.

### 9.3 Automatic `kind` mapping in `shellReducer`

| Action | Kind |
|---|---|
| `intent_submitted` | `user` |
| `preview_ready` | `system` |
| `request_approval` | `system` |
| `approval_granted` | `user` |
| `job_completed: "succeeded"` | `success` |
| `job_completed: "needs_reboot"` | `warning` |
| `job_completed: "failed"` | `error` |
| `job_completed: "rolled_back"` | `warning` |
| `cancel_requested` | `user` |
| `job_completed: "canceled"` | `warning` |
| `daemon_status_changed` | `system` |
| `plan_errored` | `error` |
| `policy_errored` | `error` |
| `timeline_event` | caller-provided `kind` |

---

## 10. Global chrome

### 10.1 Header layout

```
LACS                                ● Awaiting your approval      [Reset]
Linux Agent Control Standard        ⚠ via ollama/mistral:7b (fallback)
                                    ● daemon: connected
```

### 10.2 Status badge (human-readable)

| `ShellMode` | Badge text |
|---|---|
| `idle` | Ready |
| `planning` | Planning... |
| `previewing` | Review plan |
| `awaiting-approval` | Awaiting your approval |
| `executing` | Executing... |
| `needs-reboot` | Done — reboot required |
| `failed` | Failed |
| `rolled-back` | Rolled back |

### 10.3 Daemon connection indicator

Three states based on last connection outcome (no polling):

| State | Dot | Label |
|---|---|---|
| No attempt yet | gray | `daemon: unknown` |
| Last operation succeeded | green | `daemon: connected` |
| Last operation failed | red | `daemon: unreachable` |

Updated on every `invoke` call outcome in `daemonBridge.ts`. Dispatches `daemon_status_changed` action to keep the reducer as the single source of truth.

### 10.4 LLM provider indicator

A `get_brain_config` Tauri command returns `{ provider: string; model: string; fallback: boolean }`. Rendered below the status badge.

Normal: `via claude-opus-4-6` (gray, small, monospace)

Fallback active: `⚠ via ollama/mistral:7b (fallback)` (amber dot, same size)

The fallback warning resolves the task-8 deferred item "Frontend event when brain config falls back silently to Ollama." The timeline records it at startup as a `kind: "warning"` entry.

### 10.5 Reset button

Visible in all modes except `executing`. Dispatches `reset` on `shellReducer`. No confirmation dialog — in `executing`, Reset is hidden and Cancel is the correct escape.

---

## 11. New Tauri commands required

| Command | Input | Output | Purpose |
|---|---|---|---|
| `get_brain_config` | — | `{ provider, model, fallback }` | LLM provider indicator |
| `cancel_job` | `{ jobId: string }` | `Result<(), ShellError>` | Cancel running job |

Existing commands changed:

| Command | Change |
|---|---|
| `plan_intent` | Returns `PlanResponse` (already does); error type changes to `ShellError` |
| `approve_preview` | Must forward hash to daemon IPC (not just emit event); returns `ShellError` on failure |

---

## 12. IPC wire protocol (from companion design session)

Stateless separate-connections model. Shell opens one connection per logical operation.

**Flow:**
1. Shell sends `Preview(RequestEnvelope)` → daemon responds with `PreviewEnvelope`
2. Connection closes
3. User approves (frontend stores `request_hash`)
4. Shell sends `Approve(ApproveRequest { request_hash, action_name, params, caller_role })` → daemon validates hash via `require_fresh_approval`, executes, responds with `ResultEnvelope`

**Daemon request enum (Rust):**
```rust
enum DaemonRequest {
    StateQuery,
    Preview(RequestEnvelope),
    Approve(ApproveRequest),
    Cancel { job_id: String },
    StatusQuery { transaction_id: String },
}

struct ApproveRequest {
    request_hash: String,
    action_name: String,
    params: Value,
    caller_role: CallerRole,
}
```

Errors returned as `ResultEnvelope` with `status: Failed` and `FailureCategory` encoded in `summary`. The `ResultEnvelope.warnings` vec carries structured error detail.

---

## 13. Deferred (out of scope for this implementation)

- Streaming command output in ExecutionPane (v2)
- `Plan::new` → `Result` instead of panic
- `ActionName` newtype
- `CuratedState` private fields + custom `Deserialize`
- Structured persistent audit log for safety-fence activations
- Timeline persistence across sessions (currently in-memory only)
- "What actions are supported?" help page linked from safety-fence error
- "How to request elevated access" help page linked from role-insufficient error
