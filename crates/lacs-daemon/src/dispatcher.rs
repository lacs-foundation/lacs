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
//! - Role is checked against the action's risk level before preview and again
//!   before execute.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::net::UnixStream;
use uuid::Uuid;

use lacs_types::{CallerRole, JobState, PreviewEnvelope, RequestEnvelope, RiskLevel};

use crate::{
    auth::highest_role_from_groups,
    executor::{build_action_spec, execute_spec},
    preview::preview_action,
    state::DaemonState,
    state_collector::{collect_state, CollectedState, CommandRunner},
    transactions::{NewTransaction, TransactionStoreError},
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
/// Uses `SO_PEERCRED` (via `peer_cred()`) to obtain the peer PID, reads
/// `/proc/{pid}/status` for the supplementary GIDs, and resolves each GID
/// to a group name via `/etc/group`. Falls back to `Observer` on any error.
pub fn resolve_caller_role(stream: &UnixStream) -> CallerRole {
    let pid: u32 = match stream.peer_cred() {
        Ok(cred) => match cred.pid() {
            Some(p) if p >= 0 => p as u32,
            _ => return CallerRole::Observer,
        },
        Err(_) => return CallerRole::Observer,
    };
    let groups = groups_for_pid(pid);
    highest_role_from_groups(groups)
}

fn groups_for_pid(pid: u32) -> Vec<String> {
    let status = match std::fs::read_to_string(format!("/proc/{pid}/status")) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    for line in status.lines() {
        if line.starts_with("Groups:") {
            let gids: Vec<u32> = line
                .trim_start_matches("Groups:")
                .split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();
            return gids
                .iter()
                .filter_map(|gid| group_name_for_gid(*gid))
                .collect();
        }
    }
    vec![]
}

fn group_name_for_gid(target_gid: u32) -> Option<String> {
    let group_file = std::fs::read_to_string("/etc/group").ok()?;
    for line in group_file.lines() {
        let mut parts = line.splitn(4, ':');
        let name = parts.next()?;
        let _password = parts.next()?;
        let gid: u32 = parts.next()?.parse().ok()?;
        if gid == target_gid {
            return Some(name.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Authorization helpers
// ---------------------------------------------------------------------------

fn min_role_for_risk(risk: &RiskLevel) -> CallerRole {
    match risk {
        RiskLevel::Low => CallerRole::Observer,
        RiskLevel::Medium => CallerRole::Dev,
        RiskLevel::High => CallerRole::Admin,
    }
}

fn role_satisfies(caller: &CallerRole, required: &CallerRole) -> bool {
    role_rank(caller) >= role_rank(required)
}

fn role_rank(role: &CallerRole) -> u8 {
    match role {
        CallerRole::Observer => 1,
        CallerRole::Dev => 2,
        CallerRole::Admin => 3,
        CallerRole::Boot => 4,
    }
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
        write!(s, "{b:02x}").unwrap();
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
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap(),
                        canonical_json(&map[*k])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{pairs}}}")
        }
        _ => serde_json::to_string(v).unwrap(),
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
    let mut framed = FramedStream::new(stream);
    loop {
        let raw = match framed.recv().await {
            Ok(bytes) => bytes,
            Err(FramingError::Io(_)) => break,            // peer closed
            Err(FramingError::MessageTooLarge(_)) => break, // framing violation
        };

        let msg: Value = match serde_json::from_slice(&raw) {
            Ok(v) => v,
            Err(_) => break, // malformed JSON
        };

        let request: DaemonRequest = match serde_json::from_value(msg) {
            Ok(r) => r,
            Err(e) => {
                let _ = send_response(
                    &mut framed,
                    &DaemonResponse::ErrorResponse {
                        request_id: String::new(),
                        category: "validation_failure".into(),
                        message: format!("unknown message type: {e}"),
                    },
                )
                .await;
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
                    &caller_role,
                    request_id,
                    action_name,
                    params,
                    approval_hash,
                )
                .await
            }
            DaemonRequest::Cancel { job_id } => {
                // MVP: cancel acknowledgement only. Active-job signaling is a follow-up.
                send_response(
                    &mut framed,
                    &DaemonResponse::ErrorResponse {
                        request_id: job_id.clone(),
                        category: "validation_failure".into(),
                        message: "cancel not yet implemented".into(),
                    },
                )
                .await
            }
        };

        if let Err(e) = result {
            eprintln!("[lacs-daemon] connection handler error: {e}");
            // Error was already sent to the client as an error_response.
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
    let collected =
        match tokio::task::spawn_blocking(move || collect_state(&*runner))
            .await
            .expect("collect_state task should not panic")
        {
            Ok(s) => s,
            Err(e) => {
                return send_response(
                    framed,
                    &DaemonResponse::ErrorResponse {
                        request_id: request_id.to_string(),
                        category: "state_collection_failed".into(),
                        message: e.to_string(),
                    },
                )
                .await;
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
            return send_response(
                framed,
                &DaemonResponse::ErrorResponse {
                    request_id: request_id.to_string(),
                    category: "validation_failure".into(),
                    message: e.to_string(),
                },
            )
            .await;
        }
    };

    // Authorize caller against action risk level.
    let required = min_role_for_risk(&spec.risk_level);
    if !role_satisfies(caller_role, &required) {
        return send_response(
            framed,
            &DaemonResponse::ErrorResponse {
                request_id: request_id.to_string(),
                category: "authorization_failure".into(),
                message: format!(
                    "action '{action_name}' requires {required:?} role; caller has {caller_role:?}"
                ),
            },
        )
        .await;
    }

    let request_hash = compute_request_hash(action_name, params);

    // Snapshot current state for the preview. collect_state uses
    // std::process::Command (blocking), so offload to the blocking thread pool.
    let runner_for_preview = Arc::clone(&runner);
    let current_state = tokio::task::spawn_blocking(move || collect_state(&*runner_for_preview))
        .await
        .expect("collect_state task should not panic")
        .map(|s| serde_json::to_value(&s).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);
    let proposed_change = json!({ "action": action_name, "params": params });

    let envelope = RequestEnvelope {
        action_name: action_name.to_string(),
        request_id: request_id.to_string(),
        params: params.clone(),
        caller_role: caller_role.clone(),
        request_hash: request_hash.clone(),
    };

    let preview = preview_action(&envelope, current_state, proposed_change);

    // Persist a pending transaction so execute can verify a prior preview.
    let new_tx = NewTransaction {
        request_id: request_id.to_string(),
        request_hash,
        action_name: action_name.to_string(),
        risk_level: spec.risk_level,
        status: JobState::Queued,
        approval_id: None,
        summary: preview.summary.clone(),
        warnings: preview.warnings.clone(),
    };

    let recorded = state
        .transactions
        .record_previewed(new_tx, preview.clone())
        .map_err(HandlerError::Transaction)?;

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

async fn handle_execute(
    framed: &mut FramedStream<UnixStream>,
    state: &DaemonState,
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
            return send_response(
                framed,
                &DaemonResponse::ErrorResponse {
                    request_id: request_id.to_string(),
                    category: "validation_failure".into(),
                    message: e.to_string(),
                },
            )
            .await;
        }
    };

    // Authorize.
    let required = min_role_for_risk(&spec.risk_level);
    if !role_satisfies(caller_role, &required) {
        return send_response(
            framed,
            &DaemonResponse::ErrorResponse {
                request_id: request_id.to_string(),
                category: "authorization_failure".into(),
                message: format!(
                    "action '{action_name}' requires {required:?} role; caller has {caller_role:?}"
                ),
            },
        )
        .await;
    }

    // Compute the canonical hash and check it matches the approval.
    let stored_hash = compute_request_hash(action_name, params);
    if let Err(e) = crate::policy::require_fresh_approval(&stored_hash, approval_hash) {
        return send_response(
            framed,
            &DaemonResponse::ErrorResponse {
                request_id: request_id.to_string(),
                category: "stale_approval".into(),
                message: e.to_string(),
            },
        )
        .await;
    }

    // Verify a prior preview exists (enforce preview-before-execute).
    let prior_tx = state
        .transactions
        .find_by_request_hash(&stored_hash)
        .map_err(HandlerError::Transaction)?;

    let transaction_id = match prior_tx {
        Some(tx) => tx.transaction_id,
        None => {
            return send_response(
                framed,
                &DaemonResponse::ErrorResponse {
                    request_id: request_id.to_string(),
                    category: "stale_approval".into(),
                    message: "no prior preview found for this action; preview before executing"
                        .into(),
                },
            )
            .await;
        }
    };

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

    // Execute the action.
    let output = execute_spec(&spec).await;

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

    // Stream stdout lines as progress events.
    if let Ok(out) = &output {
        for line in out.stdout.lines() {
            if !line.is_empty() {
                let _ = send_response(
                    framed,
                    &DaemonResponse::JobProgress {
                        job_id: job_id.clone(),
                        line: line.to_string(),
                    },
                )
                .await;
            }
        }
    }

    // Update the transaction record.
    let _ = state
        .transactions
        .update_status(&transaction_id, final_status.clone());

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

    #[error("state collection failed: {0}")]
    StateCollection(#[from] crate::state_collector::CollectorError),

    #[error("transaction store error: {0}")]
    Transaction(#[from] TransactionStoreError),
}

async fn send_response(
    framed: &mut FramedStream<UnixStream>,
    response: &DaemonResponse,
) -> Result<(), HandlerError> {
    let json = serde_json::to_vec(response).expect("DaemonResponse is always serialisable");
    framed.send(&json).await.map_err(HandlerError::Framing)
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
        let db_path = dir.path().join("lacs-test.db");
        let sock_path = dir.path().join("lacs-test.sock");
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
        assert_eq!(hash.len(), 64, "request_hash should be a 64-char hex SHA-256");
        assert!(
            !resps[0]["transaction_id"].as_str().unwrap().is_empty(),
            "transaction_id must be set"
        );
    }

    #[tokio::test]
    async fn preview_hash_is_deterministic() {
        // The same action + params must always produce the same hash.
        let hash1 = compute_request_hash("InstallFlatpak", &json!({"app_id": "org.gnome.Builder", "remote": "flathub"}));
        let hash2 = compute_request_hash("InstallFlatpak", &json!({"remote": "flathub", "app_id": "org.gnome.Builder"}));
        assert_eq!(hash1, hash2, "canonical JSON must sort keys");
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
                "params": {"app_id": "org.gnome.Builder", "remote": "flathub"}
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
}
