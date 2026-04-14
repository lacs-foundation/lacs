//! Coverage-gap tests for the daemon.
//!
//! Covers: TransactionStore limit capping, combined filters, empty-hash
//! rejection, ListJobHistory via handle_query_action, bad params for
//! ListJobHistory, non-Observer action blocked by query_action, and
//! execute path authorization checks.

use std::io;
use std::sync::Arc;

use lacs_daemon::dispatcher::connection_handler_with_executor;
use lacs_daemon::actions::ActionSpec;
use lacs_daemon::executor::{ActionExecutor, ExecutionOutput, ExecutorError};
use lacs_daemon::state::{DaemonConfig, DaemonState};
use lacs_daemon::state_collector::CommandRunner;
use lacs_daemon::transport::{framing::FramedStream, grpc::ListenTarget};
use lacs_daemon::transactions::{NewTransaction, TransactionStore};
use lacs_types::{CallerRole, JobState, RiskLevel};
use serde_json::{json, Value};
use tempfile::tempdir;
use tokio::net::UnixStream;

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

struct MockRunner;

impl CommandRunner for MockRunner {
    fn run(&self, _program: &str, _args: &[&str]) -> Result<String, io::Error> {
        Ok(String::new())
    }
}

struct MockExecutor {
    exit_code: i32,
}

#[async_trait::async_trait]
impl ActionExecutor for MockExecutor {
    async fn execute(&self, _spec: &ActionSpec) -> Result<ExecutionOutput, ExecutorError> {
        Ok(ExecutionOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: self.exit_code,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_state(dir: &tempfile::TempDir) -> DaemonState {
    let db_path = dir.path().join("test.db");
    let sock_path = dir.path().join("test.sock");
    let config = DaemonConfig::new(ListenTarget::Unix(sock_path), db_path);
    DaemonState::open(config).unwrap()
}

async fn spawn_handler_with_role(
    state: DaemonState,
    role: CallerRole,
) -> FramedStream<UnixStream> {
    let (client, server) = UnixStream::pair().unwrap();
    let runner: Arc<dyn CommandRunner + Send + Sync> = Arc::new(MockRunner);
    let executor: Arc<dyn ActionExecutor> =
        Arc::new(MockExecutor { exit_code: 0 });
    tokio::spawn(async move {
        connection_handler_with_executor(server, state, runner, executor, role).await;
    });
    FramedStream::new(client)
}

async fn query_action(
    framed: &mut FramedStream<UnixStream>,
    action_name: &str,
    params: Value,
    request_id: &str,
) -> Value {
    let req = json!({
        "type": "query_action",
        "request_id": request_id,
        "action_name": action_name,
        "params": params,
    });
    framed.send(&serde_json::to_vec(&req).unwrap()).await.unwrap();
    let raw = framed.recv().await.unwrap();
    serde_json::from_slice(&raw).unwrap()
}

fn make_transaction(action: &str) -> NewTransaction {
    NewTransaction {
        request_id: format!("req-{action}"),
        request_hash: format!("hash-{action}"),
        action_name: action.to_string(),
        risk_level: RiskLevel::High,
        approval_id: None,
        summary: format!("Test {action}"),
        warnings: vec![],
    }
}

// ---------------------------------------------------------------------------
// TransactionStore: limit capping
// ---------------------------------------------------------------------------

#[test]
fn list_transactions_limit_is_capped_at_100() {
    let dir = tempdir().unwrap();
    let store =
        TransactionStore::open(dir.path().join("tx.sqlite")).expect("open store");

    // Insert 110 transactions.
    for i in 0..110 {
        store
            .record(NewTransaction {
                request_id: format!("req-{i}"),
                request_hash: format!("hash-{i}"),
                action_name: "UpdateSystem".into(),
                risk_level: RiskLevel::High,
                approval_id: None,
                summary: format!("tx {i}"),
                warnings: vec![],
            })
            .expect("record tx");
    }

    // Request 200 — must be capped to 100.
    let rows = store
        .list_transactions(200, None, None, None)
        .expect("list");
    assert!(
        rows.len() <= 100,
        "expected at most 100 rows (cap), got {}",
        rows.len()
    );
    assert_eq!(rows.len(), 100, "should return exactly 100 when 110 exist");
}

// ---------------------------------------------------------------------------
// TransactionStore: combined filter
// ---------------------------------------------------------------------------

#[test]
fn list_transactions_combined_action_and_status_filter() {
    let dir = tempdir().unwrap();
    let store =
        TransactionStore::open(dir.path().join("tx-filter.sqlite")).expect("open store");

    // Insert different actions with default Queued status.
    store.record(make_transaction("UpdateSystem")).unwrap();
    store.record(make_transaction("UpdateSystem")).unwrap();
    store.record(make_transaction("RebootSystem")).unwrap();

    // Promote one UpdateSystem to Succeeded via update_status.
    let recs = store.list_transactions(10, None, None, None).unwrap();
    let update_id = recs
        .iter()
        .find(|r| r.action_name == "UpdateSystem")
        .unwrap()
        .transaction_id
        .clone();
    store.update_status(&update_id, JobState::Running).unwrap();
    store.update_status(&update_id, JobState::Succeeded).unwrap();

    // Filter: action=UpdateSystem + status=succeeded → exactly 1 row.
    let filtered = store
        .list_transactions(100, Some("succeeded"), Some("UpdateSystem"), None)
        .expect("list filtered");
    assert_eq!(filtered.len(), 1, "expected 1 succeeded UpdateSystem");
    assert_eq!(filtered[0].action_name, "UpdateSystem");
    assert_eq!(filtered[0].status, JobState::Succeeded);
}

// ---------------------------------------------------------------------------
// approval_matches_request: empty hash edge case
// ---------------------------------------------------------------------------

#[test]
fn approval_matches_request_rejects_empty_request_hash() {
    // A SHA256 request hash is never empty; defensive guard must reject it.
    assert!(
        !lacs_daemon::policy::approval_matches_request("", ""),
        "empty request hash must not match anything"
    );
    assert!(
        !lacs_daemon::policy::approval_matches_request("", "some-hash"),
        "empty request hash must not match a non-empty approval hash"
    );
}

// ---------------------------------------------------------------------------
// ListJobHistory via handle_query_action (integration)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_job_history_returns_recorded_transactions() {
    let dir = tempdir().unwrap();
    let state = test_state(&dir);

    // Insert a transaction directly before spawning the handler.
    state
        .transactions
        .record(NewTransaction {
            request_id: "req-history".into(),
            request_hash: "hash-history".into(),
            action_name: "UpdateSystem".into(),
            risk_level: RiskLevel::High,
            approval_id: None,
            summary: "Stage system update".into(),
            warnings: vec![],
        })
        .expect("record tx");

    let mut framed = spawn_handler_with_role(state, CallerRole::Observer).await;
    let resp = query_action(
        &mut framed,
        "ListJobHistory",
        json!({ "limit": 10 }),
        "history-req",
    )
    .await;

    assert_eq!(
        resp["type"], "query_action_response",
        "expected query_action_response, got: {resp}"
    );
    let output = resp["output"].as_str().unwrap();
    assert!(
        output.contains("UpdateSystem"),
        "history output should mention UpdateSystem: {output}"
    );
}

// ---------------------------------------------------------------------------
// ListJobHistory: bad params rejected with validation_failure
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_job_history_rejects_non_integer_limit() {
    let dir = tempdir().unwrap();
    let state = test_state(&dir);
    let mut framed = spawn_handler_with_role(state, CallerRole::Observer).await;
    let resp = query_action(
        &mut framed,
        "ListJobHistory",
        json!({ "limit": "not-a-number" }),
        "bad-limit-req",
    )
    .await;

    assert_eq!(
        resp["type"], "error_response",
        "expected error_response for bad limit, got: {resp}"
    );
    assert_eq!(
        resp["category"], "validation_failure",
        "expected validation_failure category, got: {resp}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_job_history_rejects_non_integer_since_hours() {
    let dir = tempdir().unwrap();
    let state = test_state(&dir);
    let mut framed = spawn_handler_with_role(state, CallerRole::Observer).await;
    let resp = query_action(
        &mut framed,
        "ListJobHistory",
        json!({ "since_hours": "yesterday" }),
        "bad-since-req",
    )
    .await;

    assert_eq!(
        resp["type"], "error_response",
        "expected error_response for bad since_hours, got: {resp}"
    );
    assert_eq!(resp["category"], "validation_failure");
}

// ---------------------------------------------------------------------------
// query_action blocks non-Observer actions
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn query_action_rejects_non_observer_action() {
    // UpdateSystem requires Admin role — query_action only allows Observer-level.
    // Even an Admin caller should be blocked from using query_action for this.
    let dir = tempdir().unwrap();
    let state = test_state(&dir);
    let mut framed = spawn_handler_with_role(state, CallerRole::Admin).await;
    let resp = query_action(
        &mut framed,
        "UpdateSystem",
        json!({}),
        "non-observer-req",
    )
    .await;

    assert_eq!(
        resp["type"], "error_response",
        "expected error_response for non-observer action, got: {resp}"
    );
    assert_eq!(
        resp["category"], "authorization_failure",
        "non-observer action must return authorization_failure, got: {resp}"
    );
}

// ---------------------------------------------------------------------------
// handle_execute: authorization check on execute path
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn execute_rejects_observer_for_admin_action() {
    // Observer role cannot execute RebootSystem (Admin-only).
    // Authorization is checked before the preview/hash check, so we can send
    // any approval_hash and still get authorization_failure first.
    let dir = tempdir().unwrap();
    let state = test_state(&dir);
    let mut framed = spawn_handler_with_role(state, CallerRole::Observer).await;

    let req = json!({
        "type": "execute",
        "request_id": "auth-test",
        "action_name": "RebootSystem",
        "params": {},
        "approval_hash": "any-hash"
    });
    framed
        .send(&serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();

    let raw = framed.recv().await.unwrap();
    let resp: Value = serde_json::from_slice(&raw).unwrap();

    assert_eq!(
        resp["type"], "error_response",
        "expected error_response for insufficient role, got: {resp}"
    );
    assert_eq!(
        resp["category"], "authorization_failure",
        "Observer executing Admin action must get authorization_failure, got: {resp}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn execute_rejects_dev_for_admin_action() {
    // Dev role cannot execute SetKernelArguments (Admin-only).
    let dir = tempdir().unwrap();
    let state = test_state(&dir);
    let mut framed = spawn_handler_with_role(state, CallerRole::Dev).await;

    let req = json!({
        "type": "execute",
        "request_id": "dev-auth-test",
        "action_name": "SetKernelArguments",
        "params": { "add": [], "remove": [] },
        "approval_hash": "any-hash"
    });
    framed
        .send(&serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();

    let raw = framed.recv().await.unwrap();
    let resp: Value = serde_json::from_slice(&raw).unwrap();

    assert_eq!(
        resp["type"], "error_response",
        "expected error_response for Dev on Admin action, got: {resp}"
    );
    assert_eq!(resp["category"], "authorization_failure");
}
