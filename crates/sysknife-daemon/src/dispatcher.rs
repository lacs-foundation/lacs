//! IPC connection handler for the daemon.
//!
//! Each accepted Unix socket connection runs in its own Tokio task.
//! `connection_handler` authenticates the caller (the accept loop resolves
//! the `CallerRole` from `SO_PEERCRED` before spawning the task), then
//! processes a stream of length-prefixed JSON requests in a loop until the
//! connection closes.
//!
//! # Security model
//!
//! - The caller role is derived from the peer process's Linux group membership
//!   via `SO_PEERCRED` + `/proc/{pid}/status` + `/etc/group`. The shell never
//!   supplies its own role.
//! - Every `execute` request must carry an `approval_hash` that exactly matches
//!   the `request_hash` returned in the preceding `preview` response. A hash
//!   mismatch or missing prior preview returns `stale_approval`.
//! - Role is checked against the per-action allowlist (see `policy.rs`) before
//!   preview and again before execute.

use std::process::Stdio;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::net::UnixStream;
use uuid::Uuid;

use sysknife_types::{CallerRole, JobState, PreviewEnvelope, RequestEnvelope};

use crate::{
    auth::highest_role_from_groups,
    executor::{
        build_action_spec, execute_spec, rollback_spec_for, ActionExecutor, ExecutionOutput,
        ExecutorError,
    },
    preview::preview_action,
    state::DaemonState,
    state_collector::{collect_state, CollectedState, CommandRunner},
    transactions::NewTransaction,
    transport::framing::{FramedStream, FramingError},
};

// ---------------------------------------------------------------------------
// Wire types — Shell → Daemon
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DaemonRequest {
    QueryState {
        request_id: String,
    },
    Preview {
        request_id: String,
        action_name: String,
        params: Value,
    },
    Execute {
        request_id: String,
        action_name: String,
        params: Value,
        approval_hash: String,
    },
    Cancel {
        job_id: String,
    },
    QueryAction {
        request_id: String,
        action_name: String,
        params: Value,
    },
    Describe {
        request_id: String,
        action_name: String,
        params: Value,
    },
}

// ---------------------------------------------------------------------------
// Wire types — Daemon → Shell
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DaemonResponse {
    StateResponse {
        request_id: String,
        state: CollectedState,
    },
    PreviewResponse {
        request_id: String,
        preview: PreviewEnvelope,
        transaction_id: String,
    },
    JobStarted {
        request_id: String,
        job_id: String,
        transaction_id: String,
    },
    JobProgress {
        job_id: String,
        line: String,
    },
    JobCompleted {
        job_id: String,
        result: JobResult,
    },
    QueryActionResponse {
        request_id: String,
        action_name: String,
        output: String,
    },
    DescribeResponse {
        request_id: String,
        command: String,
        risk_level: String,
        reboot_required: bool,
    },
    ErrorResponse {
        request_id: String,
        category: String,
        message: String,
    },
}

#[derive(Debug, Serialize)]
struct JobResult {
    status: String,
    summary: String,
    warnings: Vec<String>,
    job_id: String,
    needs_reboot: bool,
    rollback_ref: Option<String>,
    transaction_id: String,
}

// ---------------------------------------------------------------------------
// Role resolution
// ---------------------------------------------------------------------------

/// Resolve the caller's `CallerRole` from the peer process's group membership.
///
/// Uses `SO_PEERCRED` (via `peer_cred()`) to obtain the peer PID and primary
/// GID, reads `/proc/{pid}/status` for the supplementary GIDs, and resolves
/// each GID to a group name via `/etc/group`. The primary GID is included
/// explicitly because Linux's `Groups:` line in `/proc/{pid}/status` lists
/// only supplementary groups — a process whose primary group is `wheel` would
/// otherwise be misclassified as `Observer`. Falls back to `Observer` on any
/// error.
pub fn resolve_caller_role(stream: &UnixStream) -> CallerRole {
    let (pid, primary_gid) = match stream.peer_cred() {
        Ok(cred) => {
            let pid = match cred.pid() {
                Some(p) if p >= 0 => p as u32,
                _ => return CallerRole::Observer,
            };
            (pid, cred.gid())
        }
        Err(e) => {
            eprintln!("[sysknife-daemon] WARNING: peer_cred() failed: {e}; defaulting to Observer");
            return CallerRole::Observer;
        }
    };
    // Read /etc/group once and build a lookup map — avoids N+1 file reads when
    // a process has many supplementary groups (one read per GID in the old code).
    let gid_map = read_gid_map();
    let mut groups = groups_for_pid(pid, &gid_map);
    // Include the primary GID from SO_PEERCRED. It is not listed in the
    // supplementary Groups: line so must be resolved and added explicitly.
    if let Some(name) = gid_map.get(&primary_gid) {
        if !groups.contains(name) {
            groups.push(name.clone());
        }
    }
    if groups.is_empty() {
        eprintln!(
            "[sysknife-daemon] WARNING: could not resolve groups for PID {pid}; defaulting to Observer"
        );
    }
    highest_role_from_groups(groups)
}

/// Read `/etc/group` once and return a `HashMap<gid, group_name>`.
/// Silently returns an empty map on I/O failure (falls back to Observer role).
fn read_gid_map() -> std::collections::HashMap<u32, String> {
    let content = match std::fs::read_to_string("/etc/group") {
        Ok(c) => c,
        Err(e) => {
            // Without /etc/group all callers will be resolved to Observer.
            // This is a misconfiguration or permission problem that must be visible.
            eprintln!("[sysknife-daemon] WARNING: cannot read /etc/group: {e}; all callers will be demoted to Observer");
            return std::collections::HashMap::new();
        }
    };
    let mut map = std::collections::HashMap::new();
    for (line_no, line) in content.lines().enumerate() {
        let mut parts = line.splitn(4, ':');
        let name = match parts.next() {
            Some(n) => n,
            None => {
                eprintln!("[sysknife-daemon] WARNING: malformed /etc/group line {line_no}: missing name field");
                continue;
            }
        };
        let _ = parts.next(); // password field
        let gid_str = match parts.next() {
            Some(g) => g,
            None => {
                eprintln!("[sysknife-daemon] WARNING: malformed /etc/group line {line_no} (group={name:?}): missing GID field");
                continue;
            }
        };
        match gid_str.parse::<u32>() {
            Ok(gid) => {
                map.insert(gid, name.to_string());
            }
            Err(_) => {
                eprintln!("[sysknife-daemon] WARNING: malformed /etc/group line {line_no} (group={name:?}): GID {gid_str:?} is not a number");
            }
        }
    }
    map
}

fn groups_for_pid(pid: u32, gid_map: &std::collections::HashMap<u32, String>) -> Vec<String> {
    let status = match std::fs::read_to_string(format!("/proc/{pid}/status")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "[sysknife-daemon] could not read /proc/{pid}/status: {e}; treating as no groups"
            );
            return vec![];
        }
    };
    for line in status.lines() {
        if line.starts_with("Groups:") {
            return line
                .trim_start_matches("Groups:")
                .split_whitespace()
                .filter_map(|s| s.parse::<u32>().ok())
                .filter_map(|gid| gid_map.get(&gid).cloned())
                .collect();
        }
    }
    // If the Groups: line is absent (unusual kernel config or namespacing),
    // the caller will be resolved to Observer via its primary GID only.
    // Log so operators can diagnose unexpected authorization failures.
    eprintln!(
        "[sysknife-daemon] WARNING: no Groups: line in /proc/{pid}/status; \
         supplementary groups unavailable — caller may be demoted to Observer"
    );
    vec![]
}

// ---------------------------------------------------------------------------
// Authorization helpers
// ---------------------------------------------------------------------------

fn authorize_action(caller: &CallerRole, action_name: &str) -> bool {
    crate::policy::action_allowed(caller, action_name)
}

// ---------------------------------------------------------------------------
// Request hash
// ---------------------------------------------------------------------------

/// Compute `SHA-256(action_name || "\x00" || canonical_json(params))`,
/// hex-encoded.
///
/// Canonical JSON serialises object keys in sorted order (recursively),
/// ensuring identical logical params always produce the same hash regardless
/// of insertion order.
pub fn compute_request_hash(action_name: &str, params: &Value) -> String {
    let canonical = canonical_json(params);
    let mut hasher = Sha256::new();
    hasher.update(action_name.as_bytes());
    hasher.update(b"\x00");
    hasher.update(canonical.as_bytes());
    let bytes = hasher.finalize();
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        // Writing to String via fmt::Write is infallible.
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn canonical_json(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let pairs = keys
                .iter()
                .map(|k| {
                    // Use Value::String's Display impl to JSON-encode the key —
                    // infallible because Rust strings are always valid UTF-8.
                    format!(
                        "{}:{}",
                        Value::String((*k).to_string()),
                        canonical_json(&map[*k])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{pairs}}}")
        }
        // Arrays preserve element order (ordering is meaningful) but recurse
        // into each element so nested objects get their keys sorted.
        Value::Array(arr) => {
            let items = arr.iter().map(canonical_json).collect::<Vec<_>>().join(",");
            format!("[{items}]")
        }
        // For scalars (null, bool, number, string) use Value's Display impl,
        // which renders valid JSON and is infallible.
        _ => format!("{v}"),
    }
}

// ---------------------------------------------------------------------------
// Connection handler
// ---------------------------------------------------------------------------

/// Handle a single accepted connection until the peer closes it.
///
/// `caller_role` MUST be resolved from `SO_PEERCRED` by the accept loop
/// before this function is called. This keeps the handler testable without
/// needing a real peer process.
///
/// Framing-level failures (too-large messages, malformed JSON frame) close
/// the connection. All application-level errors return an `error_response`
/// and keep the connection open.
pub async fn connection_handler(
    stream: UnixStream,
    state: DaemonState,
    runner: Arc<dyn CommandRunner + Send + Sync>,
    caller_role: CallerRole,
) {
    let executor: Arc<dyn ActionExecutor> = Arc::new(crate::executor::RealActionExecutor);
    connection_handler_with_executor(stream, state, runner, executor, caller_role).await;
}

/// Inner handler that accepts an explicit [`ActionExecutor`].
///
/// Production code enters via [`connection_handler`], which injects a
/// [`RealActionExecutor`](crate::executor::RealActionExecutor). Integration
/// tests call this directly with a mock executor to control command outcomes.
pub async fn connection_handler_with_executor(
    stream: UnixStream,
    state: DaemonState,
    runner: Arc<dyn CommandRunner + Send + Sync>,
    executor: Arc<dyn ActionExecutor>,
    caller_role: CallerRole,
) {
    let mut framed = FramedStream::new(stream);
    loop {
        let raw = match framed.recv().await {
            Ok(bytes) => bytes,
            Err(FramingError::Io(_)) => break, // peer closed
            Err(FramingError::MessageTooLarge(_)) => break, // framing violation
        };

        let msg: Value = match serde_json::from_slice(&raw) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[sysknife-daemon] malformed JSON from client, closing connection: {e}");
                break;
            }
        };

        let request: DaemonRequest = match serde_json::from_value(msg) {
            Ok(r) => r,
            Err(e) => {
                if let Err(send_err) = send_error(
                    &mut framed,
                    "",
                    "validation_failure",
                    format!("unknown message type: {e}"),
                )
                .await
                {
                    eprintln!(
                        "[sysknife-daemon] failed to send validation error response: {send_err}"
                    );
                }
                continue;
            }
        };

        let result = match &request {
            DaemonRequest::QueryState { request_id } => {
                handle_query_state(&mut framed, Arc::clone(&runner), request_id).await
            }
            DaemonRequest::Preview {
                request_id,
                action_name,
                params,
            } => {
                handle_preview(
                    &mut framed,
                    &state,
                    Arc::clone(&runner),
                    &caller_role,
                    request_id,
                    action_name,
                    params,
                )
                .await
            }
            DaemonRequest::Execute {
                request_id,
                action_name,
                params,
                approval_hash,
            } => {
                handle_execute(
                    &mut framed,
                    &state,
                    Arc::clone(&executor),
                    &caller_role,
                    request_id,
                    action_name,
                    params,
                    approval_hash,
                )
                .await
            }
            DaemonRequest::QueryAction {
                request_id,
                action_name,
                params,
            } => {
                handle_query_action(
                    &mut framed,
                    Arc::clone(&executor),
                    &state.transactions,
                    &caller_role,
                    action_name,
                    params,
                    request_id,
                )
                .await
            }
            DaemonRequest::Describe {
                request_id,
                action_name,
                params,
            } => handle_describe(&mut framed, &action_name, &params, request_id).await,
            DaemonRequest::Cancel { job_id } => {
                // MVP: cancel acknowledgement only. Active-job signaling is a follow-up.
                send_error(
                    &mut framed,
                    job_id,
                    "not_implemented",
                    "cancel not yet implemented",
                )
                .await
            }
        };

        if let Err(e) = result {
            // A framing error occurred while sending a response. Log it and
            // continue; the next recv() will return Err if the peer is gone.
            eprintln!("[sysknife-daemon] connection handler send error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Per-request handlers
// ---------------------------------------------------------------------------

async fn handle_query_state(
    framed: &mut FramedStream<UnixStream>,
    runner: Arc<dyn CommandRunner + Send + Sync>,
    request_id: &str,
) -> Result<(), HandlerError> {
    // collect_state uses std::process::Command (blocking). Offload to the
    // blocking thread pool so the async executor is not stalled.
    let join_result = tokio::task::spawn_blocking(move || collect_state(&*runner))
        .await
        .map_err(|e| HandlerError::Internal(format!("collect_state task failed: {e}")))?;
    let collected = match join_result {
        Ok(s) => s,
        Err(e) => {
            return send_error(framed, request_id, "state_collection_failed", e.to_string()).await;
        }
    };
    send_response(
        framed,
        &DaemonResponse::StateResponse {
            request_id: request_id.to_string(),
            state: collected,
        },
    )
    .await
}

async fn handle_query_action(
    framed: &mut FramedStream<UnixStream>,
    executor: Arc<dyn ActionExecutor>,
    transactions: &crate::transactions::TransactionStore,
    caller_role: &CallerRole,
    action_name: &str,
    params: &Value,
    request_id: &str,
) -> Result<(), HandlerError> {
    use crate::auth::role_rank;
    use crate::policy::min_role_for_action;

    // Defense-in-depth: verify the caller has at least Observer rank.
    if role_rank(caller_role) < role_rank(&CallerRole::Observer) {
        return send_error(
            framed,
            request_id,
            "authorization_failure",
            format!("query_action requires at least Observer role; caller has {caller_role:?}"),
        )
        .await;
    }

    // Only allow Low-risk (Observer-level) actions.
    let min_role = match min_role_for_action(action_name) {
        Some(role) => role,
        None => {
            return send_error(
                framed,
                request_id,
                "validation_failure",
                format!("unknown action: {action_name}"),
            )
            .await;
        }
    };

    if min_role != CallerRole::Observer {
        return send_error(
            framed,
            request_id,
            "authorization_failure",
            format!("{action_name} is not a read-only action; use preview+execute instead"),
        )
        .await;
    }

    // Special case: ListJobHistory queries the daemon's own transaction
    // store rather than executing a system command. Handle it here to
    // avoid routing through the ActionSpec/executor path.
    if action_name == "ListJobHistory" {
        let limit = match params.get("limit") {
            Some(v) => match v.as_u64() {
                Some(n) => n as u32,
                None => {
                    return send_error(
                        framed,
                        request_id,
                        "validation_failure",
                        format!("'limit' must be an integer, got: {v}"),
                    )
                    .await;
                }
            },
            None => 20,
        };
        let status_filter = params
            .get("status_filter")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let action_filter = params
            .get("action_filter")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let since_hours = match params.get("since_hours") {
            Some(v) => match v.as_u64() {
                Some(n) => Some(n as u32),
                None => {
                    return send_error(
                        framed,
                        request_id,
                        "validation_failure",
                        format!("'since_hours' must be an integer, got: {v}"),
                    )
                    .await;
                }
            },
            None => None,
        };

        let records = match transactions.list_transactions(
            limit,
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

        let output = if records.is_empty() {
            let mut msg = "No transactions found".to_string();
            let mut filters = Vec::new();
            if let Some(s) = &status_filter {
                filters.push(format!("status={s}"));
            }
            if let Some(a) = &action_filter {
                filters.push(format!("action={a}"));
            }
            if let Some(h) = since_hours {
                filters.push(format!("since_hours={h}"));
            }
            if !filters.is_empty() {
                msg.push_str(&format!(" (filters: {})", filters.join(", ")));
            }
            msg.push('.');
            msg
        } else {
            format_job_history(&records)
        };
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

    let spec = match build_action_spec(action_name, params) {
        Ok(s) => s,
        Err(e) => {
            return send_error(framed, request_id, "validation_failure", e.to_string()).await;
        }
    };

    let output = match executor.execute(&spec).await {
        Ok(out) => out,
        Err(e) => {
            return send_error(framed, request_id, "execution_failure", e.to_string()).await;
        }
    };

    // Non-zero exit codes from read-only actions must not be silently presented
    // as successful output. The LLM would receive error text as if it were valid
    // system state, leading to incorrect planning decisions.
    //
    // Some commands use non-zero exit codes as semantic signals rather than
    // error indicators.  These are whitelisted here so the informative stdout
    // is passed through to the caller instead of being discarded.
    //
    // - systemctl status <unit>: exits 1 when inactive, 3 when dead/failed, 4 when not
    //   found.  All produce informative output the planner needs for diagnosis.
    let is_informational_exit =
        matches!((action_name, output.exit_code), ("GetServiceStatus", 1..=4));

    if output.exit_code != 0 && !is_informational_exit {
        return send_error(
            framed,
            request_id,
            "execution_failure",
            format!(
                "{action_name} failed with exit code {}{}",
                output.exit_code,
                if output.stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", output.stderr.trim())
                }
            ),
        )
        .await;
    }

    let output_text = if output.stderr.is_empty() {
        output.stdout
    } else {
        format!("{}\n[stderr]\n{}", output.stdout, output.stderr)
    };

    send_response(
        framed,
        &DaemonResponse::QueryActionResponse {
            request_id: request_id.to_string(),
            action_name: action_name.to_string(),
            output: output_text,
        },
    )
    .await
}

/// Return a human-readable command string for an action without executing it.
///
/// Resolves `build_action_spec(action_name, params)` and formats the
/// `ActionMechanism` as a shell-style string so callers can show the user
/// exactly what will run.
async fn handle_describe(
    framed: &mut FramedStream<UnixStream>,
    action_name: &str,
    params: &Value,
    request_id: &str,
) -> Result<(), HandlerError> {
    use crate::actions::ActionMechanism;
    use crate::executor::build_action_spec;

    // ListJobHistory is handled directly in the dispatcher (SQLite query) and
    // has no ActionSpec.  Return a synthetic describe response so callers get a
    // meaningful `command` string instead of a validation_failure error.
    if action_name == "ListJobHistory" {
        return send_response(
            framed,
            &DaemonResponse::DescribeResponse {
                request_id: request_id.to_string(),
                command: "query daemon job history (SQLite)".to_string(),
                risk_level: "low".to_string(),
                reboot_required: false,
            },
        )
        .await;
    }

    let spec = match build_action_spec(action_name, params) {
        Ok(s) => s,
        Err(e) => {
            return send_error(
                framed,
                request_id,
                "validation_failure",
                format!("unknown action: {action_name} ({e})"),
            )
            .await;
        }
    };

    let command = match &spec.mechanism {
        ActionMechanism::Command { program, args } => {
            if args.is_empty() {
                program.to_string()
            } else {
                format!("{} {}", program, args.join(" "))
            }
        }
        ActionMechanism::FileScan { path } => format!("read {path}"),
        ActionMechanism::FileWrite { path, .. } => format!("write {path}"),
        ActionMechanism::FilePatch { path, .. } => format!("patch {path}"),
        ActionMechanism::FileDelete { path } => format!("rm {path}"),
    };

    let risk_level = format!("{:?}", spec.risk_level).to_lowercase();

    send_response(
        framed,
        &DaemonResponse::DescribeResponse {
            request_id: request_id.to_string(),
            command,
            risk_level,
            reboot_required: spec.reboot_required,
        },
    )
    .await
}

async fn handle_preview(
    framed: &mut FramedStream<UnixStream>,
    state: &DaemonState,
    runner: Arc<dyn CommandRunner + Send + Sync>,
    caller_role: &CallerRole,
    request_id: &str,
    action_name: &str,
    params: &Value,
) -> Result<(), HandlerError> {
    // Validate action name and params.
    let spec = match build_action_spec(action_name, params) {
        Ok(s) => s,
        Err(e) => {
            return send_error(framed, request_id, "validation_failure", e.to_string()).await;
        }
    };

    // Authorize caller against the per-action allowlist.
    if !authorize_action(caller_role, action_name) {
        return send_error(
            framed,
            request_id,
            "authorization_failure",
            format!("action '{action_name}' is not allowed for {caller_role:?} role"),
        )
        .await;
    }

    let request_hash = compute_request_hash(action_name, params);

    // Snapshot current state for the preview. collect_state uses
    // std::process::Command (blocking), so offload to the blocking thread pool.
    // State is best-effort: if collection fails, the preview is generated with
    // an empty state and a warning is logged rather than aborting the preview.
    let runner_for_preview = Arc::clone(&runner);
    let current_state =
        match tokio::task::spawn_blocking(move || collect_state(&*runner_for_preview)).await {
            Err(e) => {
                eprintln!(
                    "[sysknife-daemon] handle_preview: collect_state task failed ({e}); \
                 generating preview with empty state"
                );
                Value::Null
            }
            Ok(Err(e)) => {
                eprintln!(
                    "[sysknife-daemon] handle_preview: state collection failed ({e}); \
                 generating preview with empty state"
                );
                Value::Null
            }
            Ok(Ok(s)) => match serde_json::to_value(&s) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                    "[sysknife-daemon] handle_preview: failed to serialize collected state ({e}); \
                     using empty state"
                );
                    Value::Null
                }
            },
        };
    let proposed_change = json!({ "action": action_name, "params": params });

    let envelope = RequestEnvelope {
        action_name: action_name.to_string(),
        request_id: request_id.to_string(),
        params: params.clone(),
        caller_role: *caller_role,
        request_hash: request_hash.clone(),
    };

    let preview = preview_action(&envelope, current_state, proposed_change);

    // Persist a pending transaction so execute can verify a prior preview.
    let new_tx = NewTransaction {
        request_id: request_id.to_string(),
        request_hash,
        action_name: action_name.to_string(),
        risk_level: spec.risk_level,
        approval_id: None,
        summary: preview.summary.clone(),
        warnings: preview.warnings.clone(),
    };

    let recorded = match state.transactions.record_previewed(new_tx, preview.clone()) {
        Ok(r) => r,
        Err(e) => {
            return send_error(
                framed,
                request_id,
                "transient_infrastructure_failure",
                format!("failed to record preview transaction: {e}"),
            )
            .await;
        }
    };

    send_response(
        framed,
        &DaemonResponse::PreviewResponse {
            request_id: request_id.to_string(),
            preview,
            transaction_id: recorded.transaction.transaction_id,
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn handle_execute(
    framed: &mut FramedStream<UnixStream>,
    state: &DaemonState,
    executor: Arc<dyn ActionExecutor>,
    caller_role: &CallerRole,
    request_id: &str,
    action_name: &str,
    params: &Value,
    approval_hash: &str,
) -> Result<(), HandlerError> {
    // Validate action + params first (cheap).
    let spec = match build_action_spec(action_name, params) {
        Ok(s) => s,
        Err(e) => {
            return send_error(framed, request_id, "validation_failure", e.to_string()).await;
        }
    };

    // Authorize against the per-action allowlist.
    if !authorize_action(caller_role, action_name) {
        return send_error(
            framed,
            request_id,
            "authorization_failure",
            format!("action '{action_name}' is not allowed for {caller_role:?} role"),
        )
        .await;
    }

    // Compute the canonical hash and check it matches the approval.
    let stored_hash = compute_request_hash(action_name, params);
    if let Err(e) = crate::policy::require_fresh_approval(&stored_hash, approval_hash) {
        return send_error(framed, request_id, "stale_approval", e.to_string()).await;
    }

    // Verify a prior preview exists (enforce preview-before-execute).
    // A lookup failure is an infrastructure error — send a response instead of
    // propagating, which would leave the client hanging with no reply.
    let prior_tx = match state.transactions.find_by_request_hash(&stored_hash) {
        Ok(tx) => tx,
        Err(e) => {
            return send_error(
                framed,
                request_id,
                "transient_infrastructure_failure",
                format!("transaction lookup failed: {e}"),
            )
            .await;
        }
    };

    let transaction_id = match prior_tx {
        Some(tx) => tx.transaction_id,
        None => {
            return send_error(
                framed,
                request_id,
                "stale_approval",
                "no prior preview found for this action; preview before executing",
            )
            .await;
        }
    };

    // Atomically claim the transaction for execution (Queued → Running).
    // This closes the TOCTOU window: two concurrent execute requests that both
    // pass find_by_request_hash cannot both proceed — the loser's UPDATE sees
    // status ≠ Queued and returns false.
    let claimed = match state.transactions.claim_for_execution(&transaction_id) {
        Ok(c) => c,
        Err(e) => {
            return send_error(
                framed,
                request_id,
                "transient_infrastructure_failure",
                format!("failed to claim transaction: {e}"),
            )
            .await;
        }
    };
    if !claimed {
        return send_error(
            framed,
            request_id,
            "stale_approval",
            "transaction already claimed by a concurrent request",
        )
        .await;
    }

    let job_id = Uuid::new_v4().to_string();

    // Announce job start.
    send_response(
        framed,
        &DaemonResponse::JobStarted {
            request_id: request_id.to_string(),
            job_id: job_id.clone(),
            transaction_id: transaction_id.clone(),
        },
    )
    .await?;

    // Lifecycle event: authorization check passed.
    let _ = send_response(
        framed,
        &DaemonResponse::JobProgress {
            job_id: job_id.clone(),
            line: format!("Authorization passed for {action_name}"),
        },
    )
    .await;

    // Lifecycle event: execution starting.
    let _ = send_response(
        framed,
        &DaemonResponse::JobProgress {
            job_id: job_id.clone(),
            line: format!("Executing {action_name}..."),
        },
    )
    .await;

    // Execute the action. Command actions stream stdout live as JobProgress
    // frames; file-operation actions complete instantly with no output.
    let output = match &spec.mechanism {
        crate::actions::ActionMechanism::Command { program, args } => {
            stream_command_with_progress(framed, &job_id, program, args).await
        }
        _ => execute_spec(&spec).await,
    };

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

    // Lifecycle event: action completed with exit code or error.
    let completion_line = match &output {
        Ok(out) => format!("{action_name} completed with exit code {}", out.exit_code),
        Err(e) => format!("{action_name} completed with error: {e}"),
    };
    let _ = send_response(
        framed,
        &DaemonResponse::JobProgress {
            job_id: job_id.clone(),
            line: completion_line,
        },
    )
    .await;

    // Attempt automatic rollback if the action failed and rollback is available.
    let (final_status, summary, rollback_ref) = attempt_rollback_if_needed(
        framed,
        &executor,
        &job_id,
        action_name,
        &spec,
        initial_status,
        initial_summary,
    )
    .await;

    // Update the transaction record. A failure here is an audit-trail loss —
    // log it and surface it as a warning in the job result so the client is
    // aware of the gap.
    let mut warnings = Vec::new();
    if let Err(e) = state
        .transactions
        .update_status(&transaction_id, final_status)
    {
        eprintln!(
            "[sysknife-daemon] failed to update transaction {transaction_id} to \
             {final_status:?}: {e}"
        );
        warnings.push(format!("audit trail update failed: {e}"));
    }

    send_response(
        framed,
        &DaemonResponse::JobCompleted {
            job_id: job_id.clone(),
            result: JobResult {
                status: job_state_str(&final_status).to_string(),
                summary,
                warnings,
                job_id: job_id.clone(),
                needs_reboot: matches!(final_status, JobState::NeedsReboot),
                rollback_ref,
                transaction_id,
            },
        },
    )
    .await
}

/// Execute a `Command` action, streaming each stdout line to `framed` as a
/// `JobProgress` frame while the process runs.
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

    // Read stderr concurrently — if we exhaust stdout first and the process
    // has filled the OS stderr buffer, the process blocks on writing stderr
    // while we wait for stdout EOF: deadlock.
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        BufReader::new(stderr)
            .read_to_end(&mut buf)
            .await
            .map(|_| buf)
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
                eprintln!("[sysknife-daemon] progress send failed (client disconnected?): {e}");
            }
        }
        stdout_buf.push_str(&line);
        stdout_buf.push('\n');
    }

    let exit_status = child.wait().await.map_err(ExecutorError::Io)?;
    let stderr_bytes = stderr_task
        .await
        .map_err(|_| ExecutorError::Io(std::io::Error::other("stderr reader task panicked")))?
        .map_err(ExecutorError::Io)?;

    Ok(ExecutionOutput {
        stdout: stdout_buf,
        stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
        exit_code: exit_status.code().unwrap_or(-1),
    })
}

/// If `status` is `Failed` and `spec.rollback_available`, attempt an
/// automatic rollback. Returns the updated `(JobState, summary, rollback_ref)`.
///
/// Sends `JobProgress` frames announcing the attempt and its outcome.
/// Send failures are logged but do not abort the rollback.
async fn attempt_rollback_if_needed(
    framed: &mut FramedStream<UnixStream>,
    executor: &Arc<dyn ActionExecutor>,
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

    eprintln!("[sysknife-daemon] {action_name} failed; attempting automatic rollback");

    let _ = send_response(
        framed,
        &DaemonResponse::JobProgress {
            job_id: job_id.to_string(),
            line: format!(
                "{action_name} failed — attempting automatic rollback via rpm-ostree rollback"
            ),
        },
    )
    .await;

    match executor.execute(&rb_spec).await {
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
                format!(
                    "{action_name} failed and was automatically rolled back to the previous deployment"
                ),
                Some("rpm-ostree rollback".to_string()),
            )
        }
        other => {
            let detail = match &other {
                Ok(o) => format!("exit code {}", o.exit_code),
                Err(e) => e.to_string(),
            };
            eprintln!("[sysknife-daemon] rollback also failed: {detail}");
            let _ = send_response(
                framed,
                &DaemonResponse::JobProgress {
                    job_id: job_id.to_string(),
                    line: format!(
                        "Rollback also failed ({detail}) — system may need manual intervention"
                    ),
                },
            )
            .await;
            (status, summary, None)
        }
    }
}

fn job_state_str(state: &JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Running => "running",
        JobState::Succeeded => "succeeded",
        JobState::Failed => "failed",
        JobState::Canceled => "canceled",
        JobState::RolledBack => "rolled_back",
        JobState::NeedsReboot => "needs_reboot",
    }
}

// ---------------------------------------------------------------------------
// Framing helpers
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
enum HandlerError {
    #[error("framing error: {0}")]
    Framing(#[from] FramingError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

fn format_job_history(records: &[sysknife_types::TransactionRecord]) -> String {
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

async fn send_response(
    framed: &mut FramedStream<UnixStream>,
    response: &DaemonResponse,
) -> Result<(), HandlerError> {
    let json = serde_json::to_vec(response)?;
    framed.send(&json).await.map_err(HandlerError::Framing)
}

async fn send_error(
    framed: &mut FramedStream<UnixStream>,
    request_id: &str,
    category: &str,
    message: impl Into<String>,
) -> Result<(), HandlerError> {
    send_response(
        framed,
        &DaemonResponse::ErrorResponse {
            request_id: request_id.to_string(),
            category: category.to_string(),
            message: message.into(),
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        state::{DaemonConfig, DaemonState},
        transport::grpc::ListenTarget,
    };
    use std::io;
    use tempfile::tempdir;

    // ------------------------------------------------------------------
    // Test helpers
    // ------------------------------------------------------------------

    struct MockRunner;
    impl CommandRunner for MockRunner {
        fn run(&self, program: &str, _args: &[&str]) -> Result<String, io::Error> {
            match program {
                "hostname" => Ok("testhost\n".to_string()),
                _ => Ok(String::new()),
            }
        }
    }

    fn test_state(dir: &tempfile::TempDir) -> DaemonState {
        let db_path = dir.path().join("sysknife-test.db");
        let sock_path = dir.path().join("sysknife-test.sock");
        let config = DaemonConfig::new(ListenTarget::Unix(sock_path), db_path);
        DaemonState::open(config).unwrap()
    }

    fn runner() -> Arc<dyn CommandRunner + Send + Sync> {
        Arc::new(MockRunner)
    }

    /// Send `requests` to a spawned handler, collect exactly `want_responses`
    /// response frames, then drop the client to signal EOF.
    async fn exchange(
        state: DaemonState,
        role: CallerRole,
        requests: Vec<Value>,
        want_responses: usize,
    ) -> Vec<Value> {
        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        tokio::spawn(async move {
            connection_handler(server, state, runner(), role).await;
        });

        let mut framed = FramedStream::new(client);
        for req in &requests {
            let bytes = serde_json::to_vec(req).unwrap();
            framed.send(&bytes).await.unwrap();
        }

        let mut responses = Vec::new();
        for _ in 0..want_responses {
            let raw = framed.recv().await.unwrap();
            responses.push(serde_json::from_slice::<Value>(&raw).unwrap());
        }
        responses
    }

    // ------------------------------------------------------------------
    // query_state
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn query_state_returns_state_response() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);

        let resps = exchange(
            state,
            CallerRole::Observer,
            vec![json!({"type": "query_state", "request_id": "r1"})],
            1,
        )
        .await;

        assert_eq!(resps[0]["type"], "state_response");
        assert_eq!(resps[0]["request_id"], "r1");
        assert_eq!(resps[0]["state"]["host_name"], "testhost");
    }

    // ------------------------------------------------------------------
    // preview
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn preview_returns_hash_and_transaction_id() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);

        let resps = exchange(
            state,
            CallerRole::Observer,
            vec![json!({
                "type": "preview",
                "request_id": "r1",
                "action_name": "GetSystemState",
                "params": {}
            })],
            1,
        )
        .await;

        assert_eq!(resps[0]["type"], "preview_response");
        assert_eq!(resps[0]["request_id"], "r1");
        let hash = resps[0]["preview"]["request_hash"].as_str().unwrap();
        assert_eq!(
            hash.len(),
            64,
            "request_hash should be a 64-char hex SHA-256"
        );
        assert!(
            !resps[0]["transaction_id"].as_str().unwrap().is_empty(),
            "transaction_id must be set"
        );
    }

    #[tokio::test]
    async fn preview_hash_is_deterministic() {
        // The same action + params must always produce the same hash.
        let hash1 = compute_request_hash(
            "InstallFlatpak",
            &json!({"app_id": "org.gnome.Builder", "remote": "flathub"}),
        );
        let hash2 = compute_request_hash(
            "InstallFlatpak",
            &json!({"remote": "flathub", "app_id": "org.gnome.Builder"}),
        );
        assert_eq!(hash1, hash2, "canonical JSON must sort keys");
    }

    #[test]
    fn canonical_json_recurses_into_arrays() {
        // Objects nested inside arrays must also have sorted keys so that
        // {"packages": [{"b": 1, "a": 2}]} and {"packages": [{"a": 2, "b": 1}]}
        // produce the same hash.
        let hash1 =
            compute_request_hash("InstallPackages", &json!({"packages": [{"b": 1, "a": 2}]}));
        let hash2 =
            compute_request_hash("InstallPackages", &json!({"packages": [{"a": 2, "b": 1}]}));
        assert_eq!(hash1, hash2, "nested object keys in arrays must be sorted");
    }

    #[test]
    fn canonical_json_preserves_array_element_order() {
        // Array element order is semantically significant ("install a then b"
        // is different from "install b then a"), so it must be preserved.
        let hash1 = compute_request_hash("Op", &json!({"items": ["a", "b"]}));
        let hash2 = compute_request_hash("Op", &json!({"items": ["b", "a"]}));
        assert_ne!(hash1, hash2, "array element order must be preserved");
    }

    // ------------------------------------------------------------------
    // authorization
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn high_risk_action_rejected_for_observer() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);

        let resps = exchange(
            state,
            CallerRole::Observer, // UpdateSystem requires Admin
            vec![json!({
                "type": "preview",
                "request_id": "r1",
                "action_name": "UpdateSystem",
                "params": {}
            })],
            1,
        )
        .await;

        assert_eq!(resps[0]["type"], "error_response");
        assert_eq!(resps[0]["category"], "authorization_failure");
    }

    #[tokio::test]
    async fn medium_risk_action_rejected_for_observer() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);

        let resps = exchange(
            state,
            CallerRole::Observer, // InstallFlatpak requires Dev
            vec![json!({
                "type": "preview",
                "request_id": "r1",
                "action_name": "InstallFlatpak",
                "params": {"username": "alice", "app_id": "org.gnome.Builder", "remote": "flathub"}
            })],
            1,
        )
        .await;

        assert_eq!(resps[0]["type"], "error_response");
        assert_eq!(resps[0]["category"], "authorization_failure");
    }

    #[tokio::test]
    async fn low_risk_action_allowed_for_observer() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);

        let resps = exchange(
            state,
            CallerRole::Observer,
            vec![json!({
                "type": "preview",
                "request_id": "r1",
                "action_name": "GetSystemState",
                "params": {}
            })],
            1,
        )
        .await;

        assert_eq!(resps[0]["type"], "preview_response");
    }

    // ------------------------------------------------------------------
    // execute — stale approval
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn execute_without_prior_preview_returns_stale_approval() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);

        let hash = compute_request_hash("GetSystemState", &json!({}));
        let resps = exchange(
            state,
            CallerRole::Observer,
            vec![json!({
                "type": "execute",
                "request_id": "r1",
                "action_name": "GetSystemState",
                "params": {},
                "approval_hash": hash   // correct hash, but no prior preview
            })],
            1,
        )
        .await;

        assert_eq!(resps[0]["type"], "error_response");
        assert_eq!(resps[0]["category"], "stale_approval");
    }

    #[tokio::test]
    async fn execute_with_wrong_hash_returns_stale_approval() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);

        let resps = exchange(
            state,
            CallerRole::Observer,
            vec![json!({
                "type": "execute",
                "request_id": "r1",
                "action_name": "GetSystemState",
                "params": {},
                "approval_hash": "thisiswronghash"
            })],
            1,
        )
        .await;

        assert_eq!(resps[0]["type"], "error_response");
        assert_eq!(resps[0]["category"], "stale_approval");
    }

    // ------------------------------------------------------------------
    // execute — full preview → execute flow
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn execute_after_preview_with_correct_hash_returns_job_completed() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);
        let (client, server) = tokio::net::UnixStream::pair().unwrap();

        tokio::spawn(async move {
            connection_handler(server, state, runner(), CallerRole::Observer).await;
        });

        let mut framed = FramedStream::new(client);

        // Step 1: preview.
        let preview_req = json!({
            "type": "preview",
            "request_id": "r1",
            "action_name": "GetSystemState",
            "params": {}
        });
        framed
            .send(&serde_json::to_vec(&preview_req).unwrap())
            .await
            .unwrap();

        let raw = framed.recv().await.unwrap();
        let preview_resp: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(preview_resp["type"], "preview_response");
        let returned_hash = preview_resp["preview"]["request_hash"]
            .as_str()
            .unwrap()
            .to_string();

        // Step 2: execute with the hash the daemon returned.
        let exec_req = json!({
            "type": "execute",
            "request_id": "r2",
            "action_name": "GetSystemState",
            "params": {},
            "approval_hash": returned_hash
        });
        framed
            .send(&serde_json::to_vec(&exec_req).unwrap())
            .await
            .unwrap();

        // Expect job_started first.
        let raw = framed.recv().await.unwrap();
        let started: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(started["type"], "job_started");
        let job_id = started["job_id"].as_str().unwrap().to_string();

        // Drain frames until job_completed (there may be job_progress frames).
        loop {
            let raw = framed.recv().await.unwrap();
            let msg: Value = serde_json::from_slice(&raw).unwrap();
            let msg_type = msg["type"].as_str().unwrap();
            match msg_type {
                "job_progress" => {
                    assert_eq!(msg["job_id"], job_id);
                }
                "job_completed" => {
                    assert_eq!(msg["job_id"], job_id);
                    let status = msg["result"]["status"].as_str().unwrap();
                    // rpm-ostree may not be available in CI; both outcomes are valid.
                    assert!(
                        matches!(status, "succeeded" | "failed" | "needs_reboot"),
                        "unexpected status: {status}"
                    );
                    break;
                }
                other => panic!("unexpected message type: {other}"),
            }
        }
    }

    // ------------------------------------------------------------------
    // describe
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn describe_returns_command_and_risk_for_known_action() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);

        let resps = exchange(
            state,
            CallerRole::Observer,
            vec![json!({
                "type": "describe",
                "request_id": "r1",
                "action_name": "GetDateTime",
                "params": {}
            })],
            1,
        )
        .await;

        assert_eq!(resps[0]["type"], "describe_response");
        assert_eq!(resps[0]["request_id"], "r1");
        assert_eq!(resps[0]["command"], "timedatectl");
        assert_eq!(resps[0]["risk_level"], "low");
        assert_eq!(resps[0]["reboot_required"], false);
    }

    #[tokio::test]
    async fn describe_returns_error_for_unknown_action() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);

        let resps = exchange(
            state,
            CallerRole::Observer,
            vec![json!({
                "type": "describe",
                "request_id": "r1",
                "action_name": "NotARealAction",
                "params": {}
            })],
            1,
        )
        .await;

        assert_eq!(resps[0]["type"], "error_response");
        assert_eq!(resps[0]["category"], "validation_failure");
    }

    // ------------------------------------------------------------------
    // unknown message type
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn unknown_message_type_returns_validation_failure() {
        let dir = tempdir().unwrap();
        let state = test_state(&dir);

        let resps = exchange(
            state,
            CallerRole::Observer,
            vec![json!({"type": "does_not_exist", "request_id": "r1"})],
            1,
        )
        .await;

        assert_eq!(resps[0]["type"], "error_response");
        assert_eq!(resps[0]["category"], "validation_failure");
    }

    // ------------------------------------------------------------------
    // stream_command_with_progress
    // ------------------------------------------------------------------

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

        assert_eq!(
            msg["type"], "job_progress",
            "expected job_progress, got: {msg}"
        );
        assert_eq!(msg["job_id"], "job-test-123");
        assert_eq!(msg["line"], "hello from stream");

        let out = handle.await.unwrap();
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout.contains("hello from stream"));
    }
}
