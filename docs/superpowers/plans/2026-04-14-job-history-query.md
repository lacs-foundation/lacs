# Job History Query Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose the daemon's SQLite transaction log through the IPC layer so the planner can answer "what has LACS done recently?" and users can see their action history.

**Architecture:** Add `list_transactions()` to `TransactionStore`, intercept `ListJobHistory` in the dispatcher's `handle_query_action` before reaching the executor (it's a DB query, not an OS command), expose it as both a `query_job_history` planning tool and a `ListJobHistory` plan action, and add a system prompt example showing when to query transaction history.

**Tech Stack:** Rust (lacs-daemon, lacs-brain, lacs-types), rusqlite, existing `TransactionRecord` type, existing IPC framing.

**Branch:** `feat/job-history-query` in a dedicated worktree.

**Depends on:** Nothing — independent of the preference memory plan.

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/lacs-daemon/src/transactions.rs` | Modify | Add `list_transactions()` with filters |
| `crates/lacs-daemon/src/dispatcher.rs` | Modify | Intercept `ListJobHistory` in `handle_query_action` before executor |
| `crates/lacs-daemon/src/policy.rs` | Modify | Add `ListJobHistory` as Observer-level |
| `crates/lacs-brain/src/planning_tools/query_tools.rs` | Modify | Add `query_job_history` tool def and mapping |
| `crates/lacs-brain/src/planning_tools/propose_plan.rs` | Modify | Add `ListJobHistory` to `KNOWN_ACTIONS` |
| `crates/lacs-brain/src/prompt.rs` | Modify | Add Example C for transaction history |
| `crates/lacs-daemon/src/executor.rs` | Modify | Add `ListJobHistory` to `build_action_spec` (routes to dispatcher special case, but needs to exist so preview doesn't reject it) |

---

### Task 1: `TransactionStore::list_transactions`

**Files:**
- Modify: `crates/lacs-daemon/src/transactions.rs`

- [ ] **Step 1: Write the failing tests**

In the `mod tests` block of `crates/lacs-daemon/src/transactions.rs`, add:

```rust
// ── list_transactions tests ───────────────────────────────────────────

#[test]
fn list_transactions_returns_empty_for_fresh_store() {
    let dir = tempdir().unwrap();
    let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
    let results = store.list_transactions(10, 0, None, None, None).unwrap();
    assert!(results.is_empty());
}

#[test]
fn list_transactions_returns_all_records_ordered_by_newest_first() {
    let dir = tempdir().unwrap();
    let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
    store.record(queued_transaction()).unwrap();

    let mut second = queued_transaction();
    second.action_name = "GetDiskUsage".to_string();
    second.risk_level = RiskLevel::Low;
    store.record(second).unwrap();

    let results = store.list_transactions(10, 0, None, None, None).unwrap();
    assert_eq!(results.len(), 2);
    // Most recent first (GetDiskUsage was recorded second).
    assert_eq!(results[0].action_name, "GetDiskUsage");
    assert_eq!(results[1].action_name, "UpdateSystem");
}

#[test]
fn list_transactions_respects_limit() {
    let dir = tempdir().unwrap();
    let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
    for _ in 0..5 {
        store.record(queued_transaction()).unwrap();
    }
    let results = store.list_transactions(3, 0, None, None, None).unwrap();
    assert_eq!(results.len(), 3);
}

#[test]
fn list_transactions_respects_offset() {
    let dir = tempdir().unwrap();
    let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
    for _ in 0..5 {
        store.record(queued_transaction()).unwrap();
    }
    let results = store.list_transactions(10, 3, None, None, None).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn list_transactions_filters_by_status() {
    let dir = tempdir().unwrap();
    let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
    let tx = store.record(queued_transaction()).unwrap();
    store
        .update_status(&tx.transaction_id, JobState::Running)
        .unwrap();
    store
        .update_status(&tx.transaction_id, JobState::Succeeded)
        .unwrap();

    // Add another that stays Queued.
    store.record(queued_transaction()).unwrap();

    let succeeded = store
        .list_transactions(10, 0, Some("succeeded"), None, None)
        .unwrap();
    assert_eq!(succeeded.len(), 1);
    assert_eq!(succeeded[0].status, JobState::Succeeded);

    let queued = store
        .list_transactions(10, 0, Some("queued"), None, None)
        .unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].status, JobState::Queued);
}

#[test]
fn list_transactions_filters_by_action_name() {
    let dir = tempdir().unwrap();
    let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();
    store.record(queued_transaction()).unwrap(); // UpdateSystem

    let mut disk = queued_transaction();
    disk.action_name = "GetDiskUsage".to_string();
    store.record(disk).unwrap();

    let results = store
        .list_transactions(10, 0, None, Some("GetDiskUsage"), None)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].action_name, "GetDiskUsage");
}

#[test]
fn list_transactions_filters_by_since_hours() {
    let dir = tempdir().unwrap();
    let store = TransactionStore::open(dir.path().join("tx.db")).unwrap();

    // Record a transaction and backdate it to 48 hours ago.
    let old = store.record(queued_transaction()).unwrap();
    let conn = store.connection().unwrap();
    conn.execute(
        "UPDATE transactions SET created_at = datetime('now', '-48 hours') \
         WHERE transaction_id = ?1",
        params![old.transaction_id],
    )
    .unwrap();

    // Record a fresh transaction.
    store.record(queued_transaction()).unwrap();

    // since_hours=24 should only return the fresh one.
    let results = store
        .list_transactions(10, 0, None, None, Some(24))
        .unwrap();
    assert_eq!(results.len(), 1);

    // since_hours=72 should return both.
    let results = store
        .list_transactions(10, 0, None, None, Some(72))
        .unwrap();
    assert_eq!(results.len(), 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p lacs-daemon -- list_transactions`
Expected: FAIL — method `list_transactions` not found

- [ ] **Step 3: Implement `list_transactions`**

In `crates/lacs-daemon/src/transactions.rs`, add the method to `impl TransactionStore`, after `cleanup_stale_queued`:

```rust
    /// List transactions with optional filters, ordered by newest first.
    ///
    /// - `limit`: max number of rows (capped at 100)
    /// - `offset`: skip this many rows
    /// - `status_filter`: if set, only return rows matching this status
    ///   (e.g. `"succeeded"`, `"failed"`, `"queued"`)
    /// - `action_filter`: if set, only return rows with this exact action name
    /// - `since_hours`: if set, only return rows created within the last N hours
    pub fn list_transactions(
        &self,
        limit: u32,
        offset: u32,
        status_filter: Option<&str>,
        action_filter: Option<&str>,
        since_hours: Option<u32>,
    ) -> Result<Vec<lacs_types::TransactionRecord>, TransactionStoreError> {
        let conn = self.connection()?;
        let limit = limit.min(100);

        let mut sql = String::from(
            "SELECT transaction_id, request_id, request_hash, action_name, \
             risk_level, status, approval_id, summary, warnings_json \
             FROM transactions WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(status) = status_filter {
            // Status is stored as JSON-serialized (e.g. `"succeeded"`).
            let status_json = format!("\"{status}\"");
            sql.push_str(" AND status = ?");
            param_values.push(Box::new(status_json));
        }

        if let Some(action) = action_filter {
            sql.push_str(" AND action_name = ?");
            param_values.push(Box::new(action.to_string()));
        }

        if let Some(hours) = since_hours {
            sql.push_str(&format!(
                " AND created_at > datetime('now', '-{hours} hours')"
            ));
        }

        sql.push_str(" ORDER BY rowid DESC LIMIT ? OFFSET ?");
        param_values.push(Box::new(limit));
        param_values.push(Box::new(offset));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            Ok(row_to_record(row))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row??);
        }
        Ok(results)
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p lacs-daemon -- list_transactions`
Expected: 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/lacs-daemon/src/transactions.rs
git commit -m "feat(daemon): add list_transactions() with status/action/time filters"
```

---

### Task 2: Daemon dispatcher — intercept `ListJobHistory`

**Files:**
- Modify: `crates/lacs-daemon/src/dispatcher.rs`
- Modify: `crates/lacs-daemon/src/policy.rs`

- [ ] **Step 1: Add `ListJobHistory` to policy**

In `crates/lacs-daemon/src/policy.rs`, add `"ListJobHistory"` to the Observer-level action list (after `"ListGroups"`):

```rust
        | "ListGroups"
        | "ListJobHistory" => CallerRole::Observer,
```

- [ ] **Step 2: Add policy test**

In the `observer_can_call_read_only_actions` test, add:

```rust
        assert!(action_allowed(&role, "ListJobHistory"));
```

- [ ] **Step 3: Intercept in `handle_query_action`**

In `crates/lacs-daemon/src/dispatcher.rs`, in `handle_query_action`, add a special case BEFORE the `build_action_spec` call (after the `min_role != CallerRole::Observer` check, around line 504):

```rust
    // Special case: ListJobHistory queries the daemon's own transaction
    // store rather than executing a system command. Handle it here to
    // avoid routing through the ActionSpec/executor path.
    if action_name == "ListJobHistory" {
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as u32;
        let offset = params
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let status_filter = params
            .get("status_filter")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let action_filter = params
            .get("action_filter")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let since_hours = params
            .get("since_hours")
            .and_then(|v| v.as_u64())
            .map(|h| h as u32);

        let store = state.transaction_store();
        let records = match store.list_transactions(
            limit,
            offset,
            status_filter.as_deref(),
            action_filter.as_deref(),
            since_hours,
        ) {
            Ok(r) => r,
            Err(e) => {
                return send_error(
                    framed,
                    request_id,
                    "execution_failure",
                    format!("failed to query transaction log: {e}"),
                )
                .await;
            }
        };

        let output = format_job_history(&records);
        return send_response(
            framed,
            &DaemonResponse::QueryActionResponse {
                request_id: request_id.to_string(),
                action_name: action_name.to_string(),
                output,
            },
        )
        .await;
    }
```

- [ ] **Step 4: Add the formatting function**

Add to `dispatcher.rs`, near the bottom (before the tests module if present):

```rust
fn format_job_history(records: &[lacs_types::TransactionRecord]) -> String {
    if records.is_empty() {
        return "No transactions found.".to_string();
    }

    let mut output = format!("Transaction history ({} entries):\n\n", records.len());
    for r in records {
        output.push_str(&format!(
            "  {}  {:30}  {:12}  {}\n",
            r.transaction_id.chars().take(8).collect::<String>(),
            r.action_name,
            format!("{:?}", r.status).to_lowercase(),
            r.summary,
        ));
    }
    output
}
```

- [ ] **Step 5: Verify `DaemonState` exposes `transaction_store()`**

Check that `state.transaction_store()` exists. If `DaemonState` does not expose the store, add a getter. Look at `crates/lacs-daemon/src/state.rs` and add if needed:

```rust
pub fn transaction_store(&self) -> &TransactionStore {
    &self.store
}
```

(The exact field name depends on the `DaemonState` struct — read the file to confirm.)

- [ ] **Step 6: Run daemon tests**

Run: `cargo test -p lacs-daemon`
Expected: All existing + new policy tests PASS

- [ ] **Step 7: Commit**

```bash
git add crates/lacs-daemon/src/dispatcher.rs crates/lacs-daemon/src/policy.rs crates/lacs-daemon/src/state.rs
git commit -m "feat(daemon): intercept ListJobHistory in dispatcher for transaction log queries"
```

---

### Task 3: Brain — `query_job_history` tool and `ListJobHistory` action

**Files:**
- Modify: `crates/lacs-brain/src/planning_tools/query_tools.rs`
- Modify: `crates/lacs-brain/src/planning_tools/propose_plan.rs`

- [ ] **Step 1: Write the failing tests**

In `crates/lacs-brain/src/planning_tools/query_tools.rs`, in `mod tests`, add:

```rust
#[test]
fn query_job_history_maps_to_list_job_history() {
    let input = serde_json::json!({"limit": 20, "since_hours": 24});
    let result = query_tool_to_action("query_job_history", &input);
    assert_eq!(
        result,
        Some((
            "ListJobHistory",
            serde_json::json!({
                "limit": 20,
                "since_hours": 24
            })
        ))
    );
}

#[test]
fn query_job_history_with_all_filters() {
    let input = serde_json::json!({
        "limit": 10,
        "status_filter": "failed",
        "action_filter": "UpdateSystem",
        "since_hours": 48
    });
    let result = query_tool_to_action("query_job_history", &input);
    assert!(result.is_some());
    let (action, params) = result.unwrap();
    assert_eq!(action, "ListJobHistory");
    assert_eq!(params["status_filter"], "failed");
    assert_eq!(params["action_filter"], "UpdateSystem");
}

#[test]
fn query_job_history_with_no_filters() {
    let result = query_tool_to_action("query_job_history", &empty_input());
    assert!(result.is_some());
    let (action, _) = result.unwrap();
    assert_eq!(action, "ListJobHistory");
}
```

In `crates/lacs-brain/src/planning_tools/propose_plan.rs`, in `mod tests`, add:

```rust
#[test]
fn list_job_history_is_accepted() {
    let input = serde_json::json!({
        "summary": "show history",
        "explanation": "shows recent LACS actions",
        "steps": [{ "action_name": "ListJobHistory", "summary": "show recent activity", "risk_level": "low", "params": {} }]
    });
    parse_proposed_plan("show history", &input).unwrap();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p lacs-brain -- query_job_history`
Expected: FAIL — `query_job_history` not matched

Run: `cargo test -p lacs-brain -- list_job_history_is_accepted`
Expected: FAIL — unknown action_name 'ListJobHistory'

- [ ] **Step 3: Add `ListJobHistory` to `KNOWN_ACTIONS`**

In `crates/lacs-brain/src/planning_tools/propose_plan.rs`, add to the `KNOWN_ACTIONS` array, in the SSH section (after `"RemoveAuthorizedKey"`):

```rust
    // Job history
    "ListJobHistory",
```

- [ ] **Step 4: Add `query_job_history` tool definition**

In `crates/lacs-brain/src/planning_tools/query_tools.rs`, add to the `query_tools()` vec (after the last `ToolDefinition`, before the closing `]`):

```rust
        ToolDefinition {
            name: "query_job_history".into(),
            description: "Show recent LACS transaction history. Use this to check what \
                          actions LACS has executed (or attempted) recently. Returns \
                          action names, statuses, and summaries."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Max number of records to return (default 20, max 100)"
                    },
                    "status_filter": {
                        "type": "string",
                        "description": "Filter by status: 'succeeded', 'failed', 'queued', 'running', 'canceled'"
                    },
                    "action_filter": {
                        "type": "string",
                        "description": "Filter by exact action name, e.g. 'AddLayeredPackage'"
                    },
                    "since_hours": {
                        "type": "integer",
                        "description": "Only return records from the last N hours"
                    }
                },
                "required": []
            }),
        },
```

- [ ] **Step 5: Add `query_job_history` to the mapping function**

In `query_tool_to_action`, add before the `_ => None` arm:

```rust
        "query_job_history" => {
            let mut params = serde_json::json!({});
            if let Some(limit) = input.get("limit") {
                params["limit"] = limit.clone();
            }
            if let Some(status) = input.get("status_filter") {
                params["status_filter"] = status.clone();
            }
            if let Some(action) = input.get("action_filter") {
                params["action_filter"] = action.clone();
            }
            if let Some(hours) = input.get("since_hours") {
                params["since_hours"] = hours.clone();
            }
            Some(("ListJobHistory", params))
        }
```

- [ ] **Step 6: Update the tool count assertion**

In the `query_tools_returns_twenty_one_definitions` test, change `21` to `22`:

```rust
    fn query_tools_returns_twenty_two_definitions() {
        let tools = query_tools();
        assert_eq!(tools.len(), 22);
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p lacs-brain -- query_job_history`
Run: `cargo test -p lacs-brain -- list_job_history_is_accepted`
Run: `cargo test -p lacs-brain -- query_tools_returns`
Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add crates/lacs-brain/src/planning_tools/query_tools.rs crates/lacs-brain/src/planning_tools/propose_plan.rs
git commit -m "feat(brain): add query_job_history tool and ListJobHistory plan action"
```

---

### Task 4: System prompt — add transaction history example

**Files:**
- Modify: `crates/lacs-brain/src/prompt.rs`

- [ ] **Step 1: Add Example C after Example B**

In `crates/lacs-brain/src/prompt.rs`, after Example B ("install vim" when vim might already be layered), add:

```
### Example C — checking past LACS activity

User: "did LACS successfully update my system recently?"

Here you need to CHECK the transaction log before answering. The user is asking
about what LACS has done, not about current system state.

1. Call `query_job_history(action_filter: "UpdateSystem", since_hours: 168)` to
   check the last week of update-related transactions.
2. Call `propose_plan` with `ListJobHistory` if the user wants to see the full
   log, or `GetSystemState` if the query answered the question and you just need
   a plan to finish.

Do NOT call `query_deployments` or `get_system_state` for this — those show
current system state, not LACS transaction history.
```

- [ ] **Step 2: Add `query_job_history` to the available tools list**

In the "Available `query_*` tools" list in the prompt, add after `query_authorized_keys`:

```
     `query_job_history` (params: `limit`, `status_filter`, `action_filter`, `since_hours`).
```

- [ ] **Step 3: Add `ListJobHistory` to the Low risk action list**

In the "Available LACS actions / Low risk" list, add `ListJobHistory`:

```
GetSystemState, CollectDiagnostics, GetDeploymentHistory, ListDeployments,
GetKernelArguments, SearchFlatpakApps, ListFlatpakRemotes, GetFlatpakAppInfo,
ListToolboxes, GetLayeredPackages, ListServices, GetServiceLogs, GetFirewallState,
GetNetworkStatus, GetDiskUsage, ListProcesses, GetMemoryInfo, GetAuthorizedKeys,
ListPackageRepositories, ListContainers, GetContainerInfo, ListUsers, ListGroups,
ListJobHistory
```

- [ ] **Step 4: Update the module doc**

Update the module doc at the top of `prompt.rs` to mention "three worked examples (A, B, and C)".

- [ ] **Step 5: Run all brain tests**

Run: `cargo test -p lacs-brain`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add crates/lacs-brain/src/prompt.rs
git commit -m "feat(prompt): add Example C for transaction history and ListJobHistory action"
```

---

### Task 5: Documentation and CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/developer-guide.md`

- [ ] **Step 1: Update CLAUDE.md**

Add a note in the "Prompt Engineering" section about Example C:

```markdown
### Example C — transaction history

Example C ("did LACS successfully update recently?") teaches the model to use
`query_job_history` for questions about past LACS actions. Without it, the model
defaults to `query_deployments` or `get_system_state`, which show current system
state — not LACS's own transaction log.
```

- [ ] **Step 2: Update developer guide**

In `docs/developer-guide.md`, in the Configuration section, add `ListJobHistory` to the description of read-only actions. In the env var table, no changes needed (ListJobHistory has no env var).

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md docs/developer-guide.md
git commit -m "docs: document ListJobHistory action and query_job_history tool"
```

---

### Task 6: E2E validation

- [ ] **Step 1: Run existing stories to verify no regressions**

Run: `ANTHROPIC_API_KEY=sk-... tests/e2e/dev-stories.sh`
Expected: Stories 1-7 PASS. The new Example C and `ListJobHistory` action must not break existing story plans.

- [ ] **Step 2: Manual verification**

Start daemon, use test CLI to send "show me recent LACS activity". Verify the plan includes `ListJobHistory`. Send "what packages did I install last week?" and verify the LLM calls `query_job_history` before proposing a plan.

---

## Self-Review

**Spec coverage:** ✅ `list_transactions()` with filters → Task 1, ✅ IPC dispatch → Task 2, ✅ policy (Observer-level) → Task 2, ✅ `query_job_history` planning tool → Task 3, ✅ `ListJobHistory` plan action → Task 3, ✅ system prompt example → Task 4, ✅ documentation → Task 5.

**Placeholder scan:** No TBDs, TODOs, or placeholders found.

**Type consistency:** `list_transactions(limit: u32, offset: u32, status_filter: Option<&str>, action_filter: Option<&str>, since_hours: Option<u32>)` — consistent between Task 1 definition and Task 2 usage. `"ListJobHistory"` — consistent across `KNOWN_ACTIONS` (Task 3), `query_tool_to_action` (Task 3), `handle_query_action` (Task 2), `min_role_for_action` (Task 2), and the prompt (Task 4). `TransactionRecord` — existing type in `lacs-types`, reused without modification.
