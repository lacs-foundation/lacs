# Execution Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `lacs-daemon`'s execution pipeline production-ready by streaming stdout live during execution, running rollback when an action fails with `rollback_available: true`, adding a per-step timeout on the shell side, and capping concurrent daemon connections.

**Architecture:** `stream_command_with_progress` in `dispatcher.rs` replaces the post-execution stdout replay for `Command` actions — it spawns the process with piped stdout/stderr, reads stdout line by line, and sends each line as a `JobProgress` frame while the process runs; `rollback_spec_for` in `executor.rs` maps action names to their rollback `ActionSpec`; `attempt_rollback_if_needed` in `dispatcher.rs` calls rollback after a failure and updates the job state; a `Semaphore` in `main.rs` caps concurrent connections to 16; `daemon_client.rs` wraps the execute read loop in a 10-minute `tokio::time::timeout`.

**Tech Stack:** Rust, Tokio (`tokio::io::AsyncBufReadExt` for line-by-line streaming, `tokio::sync::Semaphore` for connection limit, `tokio::time::timeout` for execute timeout), `std::process::Stdio` for process I/O wiring.

---

## File Map

| File | Change |
|---|---|
| `crates/lacs-daemon/src/executor.rs` | Add `pub fn rollback_spec_for(action_name: &str) -> Option<ActionSpec>` |
| `crates/lacs-daemon/src/dispatcher.rs` | Add `stream_command_with_progress`, add `attempt_rollback_if_needed`, update `handle_execute` to use both, remove post-execution stdout replay |
| `crates/lacs-daemon/src/main.rs` | Add `Arc<Semaphore>` connection cap (16 slots); drop connection if limit reached |
| `apps/lacs-shell/src-tauri/src/daemon_client.rs` | Wrap execute read loop in `timeout(600s, ...)` |

---

## Task 1: Live stdout streaming

**Context:** `handle_execute` currently calls `execute_spec(&spec).await`, which waits for the process to finish and captures all output, then replays stdout lines as `JobProgress` frames. For a 2-minute `rpm-ostree update` the shell shows nothing. We need to stream stdout line by line *during* execution.

**Design:** Add `stream_command_with_progress(framed, job_id, program, args)` to `dispatcher.rs`. It spawns the process with piped stdout/stderr, reads stdout line-by-line in an async loop (sending each non-empty line as `JobProgress`), spawns a concurrent task for stderr (to avoid deadlock if stderr buffer fills), then waits for process exit and returns `ExecutionOutput`. In `handle_execute`, replace `execute_spec` with a match: `Command` arms call `stream_command_with_progress`; file-operation arms call `execute_spec` unchanged (they complete instantly with no output). The existing post-execution `JobProgress` replay loop is removed.

**Files:**
- Modify: `crates/lacs-daemon/src/dispatcher.rs`

- [ ] **Step 1: Write the failing test**

Add inside `mod tests` in `dispatcher.rs` (after the existing tests):

```rust
#[tokio::test]
async fn stream_command_sends_job_progress_lines_during_execution() {
    // stream_command_with_progress must emit JobProgress frames for each
    // stdout line WHILE the process runs, not after it exits.
    let (client_stream, server_stream) = tokio::net::UnixStream::pair().unwrap();
    let mut framed_server = FramedStream::new(server_stream);

    // Run echo in a spawned task — it exits after producing output.
    let handle = tokio::spawn(async move {
        stream_command_with_progress(
            &mut framed_server,
            "job-test-123",
            "echo",
            &["hello from stream".to_string()],
        )
        .await
        .unwrap()
    });

    // The client should receive a JobProgress frame before the task joins.
    let mut framed_client = FramedStream::new(client_stream);
    let raw = framed_client.recv().await.unwrap();
    let msg: serde_json::Value = serde_json::from_slice(&raw).unwrap();

    assert_eq!(msg["type"], "job_progress", "expected job_progress, got: {msg}");
    assert_eq!(msg["job_id"], "job-test-123");
    assert_eq!(msg["line"], "hello from stream");

    let out = handle.await.unwrap();
    assert_eq!(out.exit_code, 0);
    assert!(out.stdout.contains("hello from stream"));
}
```

- [ ] **Step 2: Run the test — confirm it fails**

```bash
cd /home/entropia/.config/superpowers/worktrees/lacs/task-11-execution-pipeline
cargo test -p lacs-daemon stream_command_sends_job_progress 2>&1 | tail -20
```

Expected: compile error `error[E0425]: cannot find function 'stream_command_with_progress'`

- [ ] **Step 3: Add `stream_command_with_progress` to `dispatcher.rs`**

Add these imports at the top of `dispatcher.rs` (after the existing `use` block):

```rust
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
```

Add the function immediately before the `// ---------------------------------------------------------------------------` separator that precedes `fn job_state_str`:

```rust
/// Execute a `Command` action spec, streaming each stdout line to `framed`
/// as a `JobProgress` frame while the process runs.
///
/// Stderr is read concurrently via a spawned task to prevent deadlock when
/// the OS stderr buffer fills before stdout is exhausted.
async fn stream_command_with_progress(
    framed: &mut FramedStream<UnixStream>,
    job_id: &str,
    program: &'static str,
    args: &[String],
) -> Result<ExecutionOutput, ExecutorError> {
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(ExecutorError::Io)?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    // Read stderr concurrently — if we read stdout until EOF first and the
    // process has also filled the OS stderr buffer, the process blocks on
    // writing stderr while we block waiting for stdout EOF: deadlock.
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        BufReader::new(stderr).read_to_end(&mut buf).await.map(|_| buf)
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut stdout_buf = String::new();

    while let Some(line) = lines.next_line().await.map_err(ExecutorError::Io)? {
        if !line.is_empty() {
            if let Err(e) = send_response(
                framed,
                &DaemonResponse::JobProgress {
                    job_id: job_id.to_string(),
                    line: line.clone(),
                },
            )
            .await
            {
                // Client disconnected mid-execution. Log and continue — the
                // daemon must not abort privileged operations because the
                // shell dropped its connection.
                eprintln!("[lacs-daemon] progress send failed (client disconnected?): {e}");
            }
        }
        stdout_buf.push_str(&line);
        stdout_buf.push('\n');
    }

    let exit_status = child.wait().await.map_err(ExecutorError::Io)?;
    let stderr_bytes = stderr_task
        .await
        .map_err(|_| {
            ExecutorError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "stderr reader task panicked",
            ))
        })?
        .map_err(ExecutorError::Io)?;

    Ok(ExecutionOutput {
        stdout: stdout_buf,
        stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
        exit_code: exit_status.code().unwrap_or(-1),
    })
}
```

Also add the missing import for `ExecutionOutput` and `ExecutorError` to the `use crate` block at the top of `dispatcher.rs`. Change:

```rust
use crate::{
    auth::highest_role_from_groups,
    executor::{build_action_spec, execute_spec},
```

to:

```rust
use crate::{
    auth::highest_role_from_groups,
    executor::{build_action_spec, execute_spec, ExecutionOutput, ExecutorError},
```

- [ ] **Step 4: Run the test — confirm it passes**

```bash
cargo test -p lacs-daemon stream_command_sends_job_progress 2>&1 | tail -10
```

Expected: `test stream_command_sends_job_progress_lines_during_execution ... ok`

- [ ] **Step 5: Wire `stream_command_with_progress` into `handle_execute`**

In `handle_execute`, find the line:

```rust
    // Execute the action.
    let output = execute_spec(&spec).await;
```

Replace it with:

```rust
    // Execute the action. Command actions stream stdout live as JobProgress
    // frames; file-operation actions complete instantly with no output.
    let output = match &spec.mechanism {
        crate::actions::ActionMechanism::Command { program, args } => {
            stream_command_with_progress(&mut framed, &job_id, program, args).await
        }
        _ => execute_spec(&spec).await,
    };
```

Then remove the existing post-execution stdout replay block (it was correct but post-hoc; now replaced by live streaming). Find and delete:

```rust
    // Stream stdout lines as progress events.
    if let Ok(out) = &output {
        for line in out.stdout.lines() {
            if !line.is_empty() {
                if let Err(e) = send_response(
                    framed,
                    &DaemonResponse::JobProgress {
                        job_id: job_id.clone(),
                        line: line.to_string(),
                    },
                )
                .await
                {
                    eprintln!("[lacs-daemon] progress send failed (client disconnected?): {e}");
                }
            }
        }
    }
```

- [ ] **Step 6: Run the full daemon test suite**

```bash
cargo test -p lacs-daemon 2>&1 | tail -20
```

Expected: all tests pass, no regressions.

- [ ] **Step 7: Commit**

```bash
git add crates/lacs-daemon/src/dispatcher.rs
git commit -m "feat(daemon): stream stdout live during execute via stream_command_with_progress

Replace post-execution stdout replay with live streaming: spawn the process
with piped stdout/stderr, read stdout line-by-line with AsyncBufReadExt, and
send each non-empty line as a JobProgress frame while the process runs.
Stderr is read concurrently in a spawned task to avoid deadlock when the OS
stderr buffer fills before stdout is exhausted.

For Command actions (e.g. rpm-ostree update), the shell now sees output
during a 2-minute install rather than a blank screen followed by a wall
of text. File-operation actions (FileWrite, FilePatch, etc.) still call
execute_spec unchanged since they complete instantly with no stdout."
```

---

## Task 2: Rollback on failure

**Context:** `JobState::RolledBack` and the shell's `rolled-back` mode exist but are never reached. When a deployment action fails, `rollback_available: true` is set on the spec, but no rollback command is issued. The shell's `RolledBackState` and the daemon's `job_state_str("rolled_back")` are dead code. We need to wire the rollback execution.

**Design:** Add `pub fn rollback_spec_for(action_name: &str) -> Option<ActionSpec>` to `executor.rs`. It returns `Some(ActionSpec)` for the five deployment actions that support rollback (`UpdateSystem`, `InstallPackages`, `RemovePackages`, `RebaseSystem`, `SetKernelArguments`) and `None` for everything else. Add `attempt_rollback_if_needed` to `dispatcher.rs` as a private async function that checks `spec.rollback_available && matches!(status, JobState::Failed)`, calls `rollback_spec_for`, runs `execute_spec`, sends progress lines about the rollback attempt, and returns the updated `(JobState, String, Option<String>)` triple. Wire it into `handle_execute` between status determination and transaction update.

**Files:**
- Modify: `crates/lacs-daemon/src/executor.rs`
- Modify: `crates/lacs-daemon/src/dispatcher.rs`

- [ ] **Step 1: Write failing tests for `rollback_spec_for` in `executor.rs`**

Add inside `#[cfg(test)] mod tests` in `executor.rs`, after the existing tests:

```rust
#[test]
fn rollback_spec_for_update_system_is_rpm_ostree_rollback() {
    let spec = rollback_spec_for("UpdateSystem").unwrap();
    assert_eq!(spec.action_name, "RollbackDeployment");
    assert!(
        matches!(
            &spec.mechanism,
            ActionMechanism::Command { program: "rpm-ostree", args }
            if args == &["rollback".to_string()]
        ),
        "expected rpm-ostree rollback, got: {:?}",
        spec.mechanism
    );
    assert!(!spec.rollback_available, "rollback must not recurse");
}

#[test]
fn rollback_spec_for_install_packages_is_rpm_ostree_rollback() {
    let spec = rollback_spec_for("InstallPackages").unwrap();
    assert_eq!(spec.action_name, "RollbackDeployment");
}

#[test]
fn rollback_spec_for_remove_packages_is_rpm_ostree_rollback() {
    assert!(rollback_spec_for("RemovePackages").is_some());
}

#[test]
fn rollback_spec_for_rebase_system_is_rpm_ostree_rollback() {
    assert!(rollback_spec_for("RebaseSystem").is_some());
}

#[test]
fn rollback_spec_for_set_kernel_arguments_is_rpm_ostree_rollback() {
    assert!(rollback_spec_for("SetKernelArguments").is_some());
}

#[test]
fn rollback_spec_for_read_only_action_returns_none() {
    assert!(rollback_spec_for("GetSystemState").is_none());
    assert!(rollback_spec_for("ListUsers").is_none());
    assert!(rollback_spec_for("GetFirewallState").is_none());
}

#[test]
fn rollback_spec_for_non_rollbackable_action_returns_none() {
    assert!(rollback_spec_for("AddUserToGroup").is_none());
    assert!(rollback_spec_for("DeleteUser").is_none());
    assert!(rollback_spec_for("CleanupDeployments").is_none());
    assert!(rollback_spec_for("RollbackDeployment").is_none()); // no infinite recursion
}
```

- [ ] **Step 2: Run — confirm failure**

```bash
cargo test -p lacs-daemon rollback_spec_for 2>&1 | tail -10
```

Expected: `error[E0425]: cannot find function 'rollback_spec_for'`

- [ ] **Step 3: Add `rollback_spec_for` to `executor.rs`**

Add after the `str_array_or_empty` function (before `#[cfg(test)]`):

```rust
/// Return the rollback `ActionSpec` for `action_name`, or `None` if no
/// automatic rollback is defined.
///
/// Only the five rpm-ostree deployment actions support rollback: they all
/// use `rpm-ostree rollback` to revert to the previous deployment. All
/// other actions either have no sensible rollback or are low-risk enough
/// that a rollback would not be beneficial.
///
/// `RollbackDeployment` itself is excluded to prevent infinite recursion.
pub fn rollback_spec_for(action_name: &str) -> Option<ActionSpec> {
    match action_name {
        "UpdateSystem" | "InstallPackages" | "RemovePackages" | "RebaseSystem"
        | "SetKernelArguments" => Some(ActionSpec {
            action_name: "RollbackDeployment",
            mechanism: ActionMechanism::Command {
                program: "rpm-ostree",
                args: vec!["rollback".to_string()],
            },
            risk_level: lacs_types::RiskLevel::High,
            reboot_required: true,
            rollback_available: false,
        }),
        _ => None,
    }
}
```

Also add `rollback_spec_for` to the imports in `dispatcher.rs`. Change:

```rust
    executor::{build_action_spec, execute_spec, ExecutionOutput, ExecutorError},
```

to:

```rust
    executor::{build_action_spec, execute_spec, rollback_spec_for, ExecutionOutput, ExecutorError},
```

- [ ] **Step 4: Run rollback_spec_for tests — confirm they pass**

```bash
cargo test -p lacs-daemon rollback_spec_for 2>&1 | tail -10
```

Expected: 8 tests pass.

- [ ] **Step 5: Add `attempt_rollback_if_needed` to `dispatcher.rs`**

Add the function immediately before `stream_command_with_progress` (both are execution helpers, group them together):

```rust
/// If `status` is `Failed` and `spec.rollback_available`, attempt an
/// automatic rollback. Returns the updated `(JobState, summary, rollback_ref)`.
///
/// Sends two `JobProgress` frames to `framed`: one announcing the rollback
/// attempt, one reporting its outcome. These are best-effort — a send failure
/// is logged but does not abort the rollback.
async fn attempt_rollback_if_needed(
    framed: &mut FramedStream<UnixStream>,
    job_id: &str,
    action_name: &str,
    spec: &crate::actions::ActionSpec,
    status: JobState,
    summary: String,
) -> (JobState, String, Option<String>) {
    if !matches!(status, JobState::Failed) || !spec.rollback_available {
        return (status, summary, None);
    }
    let Some(rb_spec) = rollback_spec_for(action_name) else {
        return (status, summary, None);
    };

    eprintln!("[lacs-daemon] {action_name} failed; attempting automatic rollback");

    let _ = send_response(
        framed,
        &DaemonResponse::JobProgress {
            job_id: job_id.to_string(),
            line: format!("{action_name} failed — attempting automatic rollback via rpm-ostree rollback"),
        },
    )
    .await;

    match execute_spec(&rb_spec).await {
        Ok(rb_out) if rb_out.exit_code == 0 => {
            let _ = send_response(
                framed,
                &DaemonResponse::JobProgress {
                    job_id: job_id.to_string(),
                    line: "Rollback succeeded — previous deployment restored".to_string(),
                },
            )
            .await;
            (
                JobState::RolledBack,
                format!("{action_name} failed and was automatically rolled back to the previous deployment"),
                Some("rpm-ostree rollback".to_string()),
            )
        }
        other => {
            let detail = match &other {
                Ok(o) => format!("exit code {}", o.exit_code),
                Err(e) => e.to_string(),
            };
            eprintln!("[lacs-daemon] rollback also failed: {detail}");
            let _ = send_response(
                framed,
                &DaemonResponse::JobProgress {
                    job_id: job_id.to_string(),
                    line: format!("Rollback also failed ({detail}) — system may need manual intervention"),
                },
            )
            .await;
            (status, summary, None)
        }
    }
}
```

- [ ] **Step 6: Wire `attempt_rollback_if_needed` into `handle_execute`**

In `handle_execute`, find the block that determines `final_status` and `summary`:

```rust
    let (final_status, summary) = match &output {
        Ok(out) if out.exit_code == 0 => {
            if spec.reboot_required {
                (
                    JobState::NeedsReboot,
                    format!("{action_name} completed; reboot required"),
                )
            } else {
                (
                    JobState::Succeeded,
                    format!("{action_name} completed successfully"),
                )
            }
        }
        Ok(out) => (
            JobState::Failed,
            format!("{action_name} failed with exit code {}", out.exit_code),
        ),
        Err(e) => (JobState::Failed, format!("{action_name} failed: {e}")),
    };
```

Replace it with:

```rust
    let (initial_status, initial_summary) = match &output {
        Ok(out) if out.exit_code == 0 => {
            if spec.reboot_required {
                (
                    JobState::NeedsReboot,
                    format!("{action_name} completed; reboot required"),
                )
            } else {
                (
                    JobState::Succeeded,
                    format!("{action_name} completed successfully"),
                )
            }
        }
        Ok(out) => (
            JobState::Failed,
            format!("{action_name} failed with exit code {}", out.exit_code),
        ),
        Err(e) => (JobState::Failed, format!("{action_name} failed: {e}")),
    };

    let (final_status, summary, rollback_ref) = attempt_rollback_if_needed(
        framed,
        &job_id,
        action_name,
        &spec,
        initial_status,
        initial_summary,
    )
    .await;
```

Then update the `JobCompleted` response to use `rollback_ref` instead of the hardcoded `None`. Find:

```rust
    send_response(
        framed,
        &DaemonResponse::JobCompleted {
            job_id: job_id.clone(),
            result: JobResult {
                status: job_state_str(&final_status).to_string(),
                summary,
                warnings: vec![],
                job_id: job_id.clone(),
                needs_reboot: matches!(final_status, JobState::NeedsReboot),
                rollback_ref: None,
                transaction_id,
            },
        },
    )
    .await
```

Replace `rollback_ref: None` with `rollback_ref`.

- [ ] **Step 7: Run full daemon tests**

```bash
cargo test -p lacs-daemon 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/lacs-daemon/src/executor.rs crates/lacs-daemon/src/dispatcher.rs
git commit -m "feat(daemon): automatic rollback when rollback_available action fails

Add rollback_spec_for() in executor.rs mapping the five rpm-ostree deployment
actions (UpdateSystem, InstallPackages, RemovePackages, RebaseSystem,
SetKernelArguments) to an rpm-ostree rollback ActionSpec.

Add attempt_rollback_if_needed() in dispatcher.rs: when handle_execute
determines JobState::Failed and spec.rollback_available is true, it attempts
the rollback, sends progress lines announcing the attempt and outcome, and
transitions to JobState::RolledBack on success. RollbackDeployment is
excluded from rollback to prevent infinite recursion.

The shell's rolled-back mode and RolledBackState were already implemented
(task-10); this commit makes that code path reachable for the first time."
```

---

## Task 3: Execute timeout + connection limit

**Context:** (a) The execute read loop in `daemon_client.rs` has no timeout — a hung daemon leaves the shell blocked indefinitely. (b) The accept loop in `main.rs` spawns one Tokio task per connection with no upper bound — a burst of connections could exhaust file descriptors. Both are simple fixes that harden production operation.

**Files:**
- Modify: `apps/lacs-shell/src-tauri/src/daemon_client.rs`
- Modify: `crates/lacs-daemon/src/main.rs`

- [ ] **Step 1: Write a test for the timeout path in `daemon_client.rs`**

The existing test mock in `daemon_client.rs` is synchronous. We can't easily simulate a hung daemon in a unit test, but we can verify the timeout constant is reasonable and the wrapping compiles. Instead, add a compile-check test that the constant is defined:

Add inside `#[cfg(test)] mod tests` in `daemon_client.rs`:

```rust
#[test]
fn execute_step_timeout_is_at_least_five_minutes() {
    // Sanity check: the timeout must be long enough for slow package operations
    // (rpm-ostree update on a slow mirror can take 5-10 minutes) but not so
    // long that a stuck daemon hangs the shell indefinitely.
    assert!(
        EXECUTE_STEP_TIMEOUT_SECS >= 300,
        "execute step timeout {EXECUTE_STEP_TIMEOUT_SECS}s is too short; rpm-ostree can take 5+ minutes"
    );
    assert!(
        EXECUTE_STEP_TIMEOUT_SECS <= 1800,
        "execute step timeout {EXECUTE_STEP_TIMEOUT_SECS}s is too long; stuck jobs should surface sooner"
    );
}
```

- [ ] **Step 2: Run — confirm failure**

```bash
cargo test -p lacs-shell execute_step_timeout 2>&1 | tail -10
```

Expected: `error[E0425]: cannot find value 'EXECUTE_STEP_TIMEOUT_SECS'`

- [ ] **Step 3: Add the constant and timeout in `daemon_client.rs`**

At the top of `daemon_client.rs`, after the existing constants:

```rust
/// Per-step timeout for the execute read loop (seconds).
///
/// `rpm-ostree update` on a slow mirror can take 5–10 minutes. We allow
/// up to 10 minutes per step. A hung daemon will be detected within this
/// window and reported to the user as a failure.
const EXECUTE_STEP_TIMEOUT_SECS: u64 = 600;
```

In `execute_action`, find the execute read loop:

```rust
    // ── Stream responses ──────────────────────────────────────────────────────
    loop {
        let raw = async_read_framed(&mut stream)
            .await
            .map_err(|e| format!("failed to read execute response: {e}"))?;
```

Replace with:

```rust
    // ── Stream responses ──────────────────────────────────────────────────────
    loop {
        let raw = timeout(
            TDuration::from_secs(EXECUTE_STEP_TIMEOUT_SECS),
            async_read_framed(&mut stream),
        )
        .await
        .map_err(|_| {
            format!(
                "daemon execute timed out after {EXECUTE_STEP_TIMEOUT_SECS}s; \
                 the job may still be running on the daemon side"
            )
        })?
        .map_err(|e| format!("failed to read execute response: {e}"))?;
```

(`timeout` and `TDuration` are already imported in `execute_action`'s scope via `use tokio::time::{timeout, Duration as TDuration};`.)

- [ ] **Step 4: Run the test — confirm it passes**

```bash
cargo test -p lacs-shell execute_step_timeout 2>&1 | tail -10
```

Expected: `test daemon_client::tests::execute_step_timeout_is_at_least_five_minutes ... ok`

- [ ] **Step 5: Write a test for the connection limit in `main.rs`**

The connection semaphore is a behavioral invariant of the accept loop and cannot easily be unit-tested without running the full daemon. Instead, add a compile-check test for the constant:

In `main.rs`, add:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn max_connections_is_reasonable() {
        assert!(
            super::MAX_CONNECTIONS >= 4,
            "MAX_CONNECTIONS too low; need at least one connection per lacs-shell instance + headroom"
        );
        assert!(
            super::MAX_CONNECTIONS <= 64,
            "MAX_CONNECTIONS too high; each connection holds a transaction DB connection"
        );
    }
}
```

- [ ] **Step 6: Run — confirm failure**

```bash
cargo test -p lacs-daemon max_connections 2>&1 | tail -10
```

Expected: `error[E0412]: cannot find type` or `error[E0425]: cannot find value 'MAX_CONNECTIONS'`

- [ ] **Step 7: Add the semaphore and constant to `main.rs`**

Add at the top of `main.rs` after the existing `use` block:

```rust
use tokio::sync::Semaphore;
```

Add the constant before `main`:

```rust
/// Maximum number of concurrent IPC connections the daemon accepts.
///
/// Each shell instance opens one connection per plan step. 16 slots allows
/// 16 concurrent shell sessions before new connections are dropped.
/// Raising this too high risks file descriptor exhaustion (EMFILE) under load.
const MAX_CONNECTIONS: usize = 16;
```

In the accept loop, replace:

```rust
                    Ok((stream, _addr)) => {
                        let role = resolve_caller_role(&stream);
                        let state = state.clone();
                        let runner = Arc::clone(&runner);
                        tokio::spawn(async move {
                            connection_handler(stream, state, runner, role).await;
                        });
                    }
```

with:

```rust
                    Ok((stream, _addr)) => {
                        match Arc::clone(&semaphore).try_acquire_owned() {
                            Ok(permit) => {
                                let role = resolve_caller_role(&stream);
                                let state = state.clone();
                                let runner = Arc::clone(&runner);
                                tokio::spawn(async move {
                                    connection_handler(stream, state, runner, role).await;
                                    drop(permit); // release slot when handler finishes
                                });
                            }
                            Err(_) => {
                                eprintln!(
                                    "[lacs-daemon] connection limit ({MAX_CONNECTIONS}) reached; \
                                     dropping new connection"
                                );
                                // Dropping stream here closes the connection immediately.
                            }
                        }
                    }
```

Before the `loop {` in `main`, add:

```rust
    let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));
```

- [ ] **Step 8: Run all tests**

```bash
cargo test --workspace 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 9: Commit**

```bash
git add apps/lacs-shell/src-tauri/src/daemon_client.rs crates/lacs-daemon/src/main.rs
git commit -m "feat(daemon,shell): execute timeout and connection limit

daemon_client.rs: wrap the execute read loop in tokio::time::timeout(600s).
A hung daemon now surfaces as a failure after 10 minutes rather than
hanging the shell indefinitely. The timeout is long enough for slow package
operations (rpm-ostree update on a slow mirror) but bounded.

main.rs: add Arc<Semaphore> with MAX_CONNECTIONS=16 slots. try_acquire_owned()
is used (non-blocking) so the accept loop is never suspended waiting for a
slot — excess connections are dropped immediately with a log message. Each
spawned task holds the permit until connection_handler returns."
```

---

## Self-Review

**Spec coverage check:**
- Live streaming ✓ (Task 1)
- Rollback on failure ✓ (Task 2)
- Execute timeout ✓ (Task 3)
- Connection limit ✓ (Task 3)

**Placeholder scan:**
- No TBD, no "implement later", no vague steps — all code is shown in full.

**Type consistency:**
- `rollback_spec_for` defined in Task 2 Step 3, imported in dispatcher in same step — consistent.
- `attempt_rollback_if_needed` defined and called in Task 2 Steps 5-6 with matching signatures.
- `EXECUTE_STEP_TIMEOUT_SECS: u64` defined and referenced in test in same task.
- `stream_command_with_progress` takes `(framed: &mut FramedStream<UnixStream>, job_id: &str, program: &'static str, args: &[String])` — matches call site in Task 1 Step 5 which passes `program` from `ActionMechanism::Command { program, args }` where `program: &'static str`.
- `ExecutionOutput` and `ExecutorError` added to imports in Task 1 Step 3 and referenced in `stream_command_with_progress` — consistent.
