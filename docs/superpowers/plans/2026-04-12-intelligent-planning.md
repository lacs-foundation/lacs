# Intelligent Planning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the LLM read-only query tools during planning and post-execution feedback so it can gather specific state before proposing and summarize results after execution.

**Architecture:** Two features sharing the same IPC path. Feature A adds a `query_action` daemon request type that runs any low-risk action and returns its stdout — no preview/approval needed. The planner gets 6 new tools that call `query_action` via the StateClient. Feature B captures execution output and feeds it back to the LLM for summarization after plan execution completes.

**Tech Stack:** Rust (lacs-brain, lacs-daemon), TypeScript/React (lacs-shell), Unix socket IPC, serde_json

---

## File Structure

### New files

- `crates/lacs-brain/src/planning_tools/query_tools.rs` — 6 read-only query tool definitions
- `crates/lacs-brain/src/planning_tools/review_execution.rs` — post-execution review tool

### Modified files

- `crates/lacs-brain/src/planning_tools/mod.rs` — export new modules
- `crates/lacs-brain/src/state_client.rs` — add `query_action()` method to `StateClient` trait
- `crates/lacs-brain/src/planner.rs` — register new tools, add query dispatch, add `review_execution()` method
- `crates/lacs-brain/src/prompt.rs` — update system prompt with new workflow
- `crates/lacs-brain/src/config.rs` — increase DEFAULT_MAX_TURNS from 5 to 10
- `crates/lacs-daemon/src/dispatcher.rs` — add `QueryAction` request handler
- `apps/lacs-shell/src-tauri/src/daemon_client.rs` — add `query_action()` IPC method
- `apps/lacs-shell/src-tauri/src/commands.rs` — capture execution output, add `review_execution` command, update DemoStateClient
- `apps/lacs-shell/src/shellState.ts` — add `reviewing` mode
- `apps/lacs-shell/src/App.tsx` — handle reviewing mode, render summary
- `apps/lacs-shell/src/components/ReviewPane.tsx` — new component for execution summary
- `apps/lacs-shell/src/daemonBridge.ts` — add `reviewExecution()` bridge function
- `apps/lacs-shell/src/types.ts` — add `ExecutionSummary` type

---

## Task 1: Add `query_action` to daemon IPC protocol

**Files:**
- Modify: `crates/lacs-daemon/src/dispatcher.rs`
- Test: existing integration tests + new unit test

The daemon gets a new request type `query_action` that runs any Low-risk action and returns stdout directly. No preview, no approval, no transaction log — this is read-only.

- [ ] **Step 1: Add QueryAction variant to DaemonRequest**

```rust
// In crates/lacs-daemon/src/dispatcher.rs, add to the DaemonRequest enum:
QueryAction {
    request_id: String,
    action_name: String,
    params: Value,
},
```

- [ ] **Step 2: Add QueryActionResponse to DaemonResponse**

```rust
// Add to DaemonResponse enum:
QueryActionResponse {
    request_id: String,
    action_name: String,
    output: String,
},
```

- [ ] **Step 3: Implement handle_query_action**

```rust
async fn handle_query_action(
    framed: &mut FramedStream<UnixStream>,
    action_name: &str,
    params: &Value,
    request_id: &str,
) -> Result<(), HandlerError> {
    // Only allow Low-risk actions
    use crate::policy::min_role_for_action;
    use lacs_types::CallerRole;

    let min_role = min_role_for_action(action_name)
        .ok_or_else(|| HandlerError::Validation(format!("unknown action: {action_name}")))?;

    if min_role != CallerRole::Observer {
        return send_error(framed, request_id, "authorization_failure",
            &format!("{action_name} is not a read-only action; use preview+execute instead"))
            .await;
    }

    let spec = build_action_spec(action_name, params)
        .map_err(|e| HandlerError::Validation(e.to_string()))?;

    let output = execute_spec(&spec).await
        .map_err(|e| HandlerError::Execution(e.to_string()))?;

    send_response(framed, &json!({
        "type": "query_action_response",
        "request_id": request_id,
        "action_name": action_name,
        "output": output.stdout,
    })).await
}
```

- [ ] **Step 4: Route QueryAction in connection_handler**

Add a match arm in the main request dispatch loop for `DaemonRequest::QueryAction`.

- [ ] **Step 5: Run `cargo check -p lacs-daemon`**

- [ ] **Step 6: Commit**

```bash
git add crates/lacs-daemon/src/dispatcher.rs
git commit -m "feat(daemon): add query_action IPC request for read-only actions"
```

---

## Task 2: Add `query_action()` to StateClient trait and DaemonIpcClient

**Files:**
- Modify: `crates/lacs-brain/src/state_client.rs`
- Modify: `apps/lacs-shell/src-tauri/src/daemon_client.rs`
- Modify: `apps/lacs-shell/src-tauri/src/commands.rs` (DemoStateClient)
- Test: `crates/lacs-brain/tests/planner.rs` (MockStateClient)

- [ ] **Step 1: Add query_action to StateClient trait**

```rust
// In crates/lacs-brain/src/state_client.rs, add to StateClient trait:
pub trait StateClient: Send + Sync {
    fn curated_state(&self) -> Result<CuratedState, PlanningError>;

    /// Run a read-only action on the daemon and return its stdout.
    /// Only Low-risk (Observer-level) actions are allowed.
    fn query_action(&self, action_name: &str, params: &serde_json::Value)
        -> Result<String, PlanningError>;
}
```

- [ ] **Step 2: Implement in DaemonIpcClient**

```rust
// In apps/lacs-shell/src-tauri/src/daemon_client.rs:
fn query_action_inner(&self, action_name: &str, params: &serde_json::Value) -> Result<String, String> {
    let mut stream = UnixStream::connect(&self.socket_path)
        .map_err(|e| format!("daemon connect: {e}"))?;
    stream.set_read_timeout(Some(SOCKET_TIMEOUT)).ok();
    stream.set_write_timeout(Some(SOCKET_TIMEOUT)).ok();

    let request = serde_json::to_vec(&serde_json::json!({
        "type": "query_action",
        "request_id": format!("query-{action_name}"),
        "action_name": action_name,
        "params": params,
    })).map_err(|e| format!("serialize: {e}"))?;

    write_framed(&mut stream, &request)
        .map_err(|e| format!("send: {e}"))?;
    let msg = read_framed(&mut stream)
        .map_err(|e| format!("read: {e}"))?;
    let resp: serde_json::Value = serde_json::from_slice(&msg)
        .map_err(|e| format!("parse: {e}"))?;

    match resp["type"].as_str() {
        Some("query_action_response") => {
            Ok(resp["output"].as_str().unwrap_or("").to_string())
        }
        Some("error_response") => Err(format!(
            "daemon error: {}", resp["message"].as_str().unwrap_or("unknown")
        )),
        other => Err(format!("unexpected: {}", other.unwrap_or("<missing>"))),
    }
}
```

Add `StateClient::query_action` implementation that calls `query_action_inner`.

- [ ] **Step 3: Implement in DemoStateClient (stub)**

```rust
fn query_action(&self, action_name: &str, _params: &serde_json::Value)
    -> Result<String, PlanningError> {
    // Return canned responses for demo/test mode
    Ok(format!("[demo] {action_name} output would appear here"))
}
```

- [ ] **Step 4: Implement in MockStateClient (tests)**

Same stub returning a static string.

- [ ] **Step 5: Run `cargo check --workspace`**

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(brain): add query_action to StateClient trait and implementations"
```

---

## Task 3: Create planning-phase query tool definitions

**Files:**
- Create: `crates/lacs-brain/src/planning_tools/query_tools.rs`
- Modify: `crates/lacs-brain/src/planning_tools/mod.rs`

- [ ] **Step 1: Create query_tools.rs with 6 tool definitions**

```rust
//! Read-only query tools for the planning phase.
//!
//! These tools let the LLM gather specific system information before
//! proposing a plan. Each maps to a Low-risk daemon action.

use crate::provider::ToolDefinition;

pub fn query_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "query_services".into(),
            description: "List all running systemd services. Returns one service name per line.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
        ToolDefinition {
            name: "query_firewall".into(),
            description: "Show current firewall rules and allowed services.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
        ToolDefinition {
            name: "query_deployments".into(),
            description: "List all rpm-ostree deployments with their index, version, and pinned status.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
        ToolDefinition {
            name: "query_packages".into(),
            description: "List all layered packages installed via rpm-ostree.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
        ToolDefinition {
            name: "query_containers".into(),
            description: "List all running containers (podman) with name and status.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
        ToolDefinition {
            name: "query_users".into(),
            description: "List local user accounts (uid >= 1000) with username and groups.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
    ]
}

/// Map a query tool name to the corresponding daemon action name + params.
pub fn query_tool_to_action(tool_name: &str) -> Option<(&'static str, serde_json::Value)> {
    match tool_name {
        "query_services" => Some(("ListServices", serde_json::json!({}))),
        "query_firewall" => Some(("GetFirewallState", serde_json::json!({}))),
        "query_deployments" => Some(("ListDeployments", serde_json::json!({}))),
        "query_packages" => Some(("GetLayeredPackages", serde_json::json!({}))),
        "query_containers" => Some(("ListContainers", serde_json::json!({}))),
        "query_users" => Some(("ListUsers", serde_json::json!({}))),
        _ => None,
    }
}
```

- [ ] **Step 2: Export from mod.rs**

```rust
pub(crate) mod get_state;
pub mod propose_plan;
pub(crate) mod query_tools;
```

- [ ] **Step 3: Run `cargo check -p lacs-brain`**

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(brain): add 6 read-only query tool definitions for planning phase"
```

---

## Task 4: Register query tools in the planner and dispatch calls

**Files:**
- Modify: `crates/lacs-brain/src/planner.rs`
- Modify: `crates/lacs-brain/src/config.rs`

- [ ] **Step 1: Register query tools in LlmPlanner::new**

```rust
// Change tools construction in new():
use crate::planning_tools::query_tools::query_tools;

let mut tools = vec![get_state_tool_def()];
tools.extend(query_tools());
tools.push(propose_plan_tool_def());

Self {
    // ...
    tools,
    // ...
}
```

- [ ] **Step 2: Add query tool dispatch in plan_intent loop**

In the tool call match block, add a new arm before the `unknown` arm:

```rust
name if crate::planning_tools::query_tools::query_tool_to_action(name).is_some() => {
    let (action_name, params) = crate::planning_tools::query_tools::query_tool_to_action(name).unwrap();
    match self.state_client.query_action(action_name, &params) {
        Ok(output) => {
            tool_results.push(ToolResultBlock {
                tool_use_id: id.clone(),
                content: output,
                is_error: false,
            });
        }
        Err(e) => {
            tool_results.push(ToolResultBlock {
                tool_use_id: id.clone(),
                content: format!("Query failed: {e}"),
                is_error: true,
            });
        }
    }
}
```

- [ ] **Step 3: Increase DEFAULT_MAX_TURNS to 10**

In `crates/lacs-brain/src/config.rs`:
```rust
pub const DEFAULT_MAX_TURNS: usize = 10;
```

The LLM now has 8 tools it might call before `propose_plan`. 5 turns is too few.

- [ ] **Step 4: Run `cargo check --workspace`**

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(brain): register query tools in planner and dispatch via StateClient"
```

---

## Task 5: Update system prompt with new workflow

**Files:**
- Modify: `crates/lacs-brain/src/prompt.rs`

- [ ] **Step 1: Update the system prompt**

Replace the Workflow section:

```rust
## Workflow

1. Call `get_system_state` to get a high-level overview of the system.
2. If you need specific details, call one or more query tools:
   - `query_services` — list running systemd services
   - `query_firewall` — show firewall rules
   - `query_deployments` — list rpm-ostree deployments
   - `query_packages` — list layered packages
   - `query_containers` — list running containers
   - `query_users` — list local user accounts
3. Call `propose_plan` exactly once with the typed plan.

You MUST call `propose_plan` to finish. Do not respond with plain text.
Gather the information you need BEFORE proposing — you cannot see execution results.
```

- [ ] **Step 2: Run `cargo check -p lacs-brain`**

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(brain): update system prompt with query tools workflow"
```

---

## Task 6: Add tests for the query tool flow

**Files:**
- Modify: `crates/lacs-brain/tests/planner.rs`
- Create: `crates/lacs-brain/src/planning_tools/query_tools.rs` (add tests)

- [ ] **Step 1: Add unit tests for query_tool_to_action mapping**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_query_tools_map_to_actions() {
        assert_eq!(query_tool_to_action("query_services"), Some(("ListServices", serde_json::json!({}))));
        assert_eq!(query_tool_to_action("query_firewall"), Some(("GetFirewallState", serde_json::json!({}))));
        assert_eq!(query_tool_to_action("query_deployments"), Some(("ListDeployments", serde_json::json!({}))));
        assert_eq!(query_tool_to_action("query_packages"), Some(("GetLayeredPackages", serde_json::json!({}))));
        assert_eq!(query_tool_to_action("query_containers"), Some(("ListContainers", serde_json::json!({}))));
        assert_eq!(query_tool_to_action("query_users"), Some(("ListUsers", serde_json::json!({}))));
    }

    #[test]
    fn unknown_query_tool_returns_none() {
        assert!(query_tool_to_action("query_unknown").is_none());
        assert!(query_tool_to_action("propose_plan").is_none());
    }

    #[test]
    fn query_tools_returns_six_definitions() {
        let tools = query_tools();
        assert_eq!(tools.len(), 6);
        for tool in &tools {
            assert!(tool.name.starts_with("query_"));
            assert!(!tool.description.is_empty());
        }
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --workspace
```

- [ ] **Step 3: Commit**

```bash
git commit -m "test(brain): add unit tests for query tool definitions and mapping"
```

---

## Task 7: Capture execution output for post-execution review

**Files:**
- Modify: `apps/lacs-shell/src-tauri/src/commands.rs`
- Modify: `apps/lacs-shell/src/types.ts`

- [ ] **Step 1: Capture step outputs in approve_preview**

Add output collection to `approve_preview`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepOutput {
    pub action_name: String,
    pub status: String,
    pub output_lines: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    pub outcome: String,
    pub step_outputs: Vec<StepOutput>,
}
```

Modify `approve_preview` to collect timeline lines per step and emit `ExecutionResult` instead of just the outcome string.

- [ ] **Step 2: Add new Tauri command review_execution**

```rust
#[tauri::command]
pub async fn review_execution(
    state: tauri::State<'_, ShellCommandState>,
    execution_result: ExecutionResult,
    intent: String,
) -> Result<String, ShellError> {
    // Build a summary prompt from the execution result
    let mut summary_parts = vec![
        format!("The user asked: \"{intent}\""),
        format!("Execution outcome: {}", execution_result.outcome),
        String::new(),
    ];
    for step in &execution_result.step_outputs {
        summary_parts.push(format!("Step: {} ({})", step.action_name, step.status));
        for line in &step.output_lines {
            summary_parts.push(format!("  {line}"));
        }
    }
    let summary_prompt = summary_parts.join("\n");

    // Call the LLM with a simple summarization prompt (no tools)
    // For now, return the raw output — full LLM summarization is a follow-up
    Ok(summary_prompt)
}
```

- [ ] **Step 3: Register command in main.rs**

- [ ] **Step 4: Add TypeScript types**

```typescript
export interface StepOutput {
  actionName: string;
  status: string;
  outputLines: string[];
}

export interface ExecutionResult {
  outcome: string;
  stepOutputs: StepOutput[];
}
```

- [ ] **Step 5: Run `cargo check -p lacs-shell`**

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(shell): capture execution output and add review_execution command"
```

---

## Task 8: Add reviewing mode to shell state machine

**Files:**
- Modify: `apps/lacs-shell/src/shellState.ts`
- Modify: `apps/lacs-shell/src/types.ts`

- [ ] **Step 1: Add reviewing mode**

```typescript
// Add to ShellMode:
| "reviewing"

// Add reviewing state variant:
| {
    mode: "reviewing";
    intent: string;
    plan: PlanResponse;
    activeJobId: null;
    executionResult: ExecutionResult;
    summary: string | null;  // LLM-generated summary, null while loading
    timeline: TimelineEntry[];
    daemonStatus: DaemonStatus;
  }
```

- [ ] **Step 2: Add new actions**

```typescript
| { type: "execution_review_ready"; result: ExecutionResult }
| { type: "summary_ready"; summary: string }
| { type: "dismiss_review" }
```

- [ ] **Step 3: Add transitions in shellReducer**

```typescript
// executing + job_completed(succeeded/needs_reboot) → reviewing
// reviewing + summary_ready → reviewing (with summary)
// reviewing + dismiss_review → idle
// reviewing + reset → idle
```

- [ ] **Step 4: Run `pnpm test`**

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(shell): add reviewing mode to state machine for post-execution feedback"
```

---

## Task 9: Create ReviewPane component

**Files:**
- Create: `apps/lacs-shell/src/components/ReviewPane.tsx`
- Modify: `apps/lacs-shell/src/App.tsx`

- [ ] **Step 1: Create ReviewPane**

Shows:
- Execution outcome badge (succeeded/failed/rolled-back/needs-reboot)
- Per-step summary (action name + status)
- LLM-generated summary (or "Generating summary..." spinner)
- "New task" button to dismiss

- [ ] **Step 2: Wire into App.tsx**

Render `ReviewPane` when `state.mode === "reviewing"`. On `job_completed`, transition to reviewing mode and call `reviewExecution()` to get the summary.

- [ ] **Step 3: Add bridge function**

```typescript
export async function reviewExecution(
  executionResult: ExecutionResult,
  intent: string,
): Promise<string> {
  return invoke<string>("review_execution", { executionResult, intent });
}
```

- [ ] **Step 4: Run `pnpm test`**

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(shell): add ReviewPane component for post-execution summary"
```

---

## Task 10: Integration test and final verification

**Files:**
- All modified files

- [ ] **Step 1: Run full Rust test suite**

```bash
cargo test --workspace
```

- [ ] **Step 2: Run frontend tests**

```bash
cd apps/lacs-shell && pnpm test
```

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --workspace --all-features --locked -- -D warnings
```

- [ ] **Step 4: Run fmt**

```bash
cargo fmt --all --check
```

- [ ] **Step 5: Commit any remaining fixes**

- [ ] **Step 6: Final commit**

```bash
git commit -m "test: verify intelligent planning implementation passes all checks"
```
