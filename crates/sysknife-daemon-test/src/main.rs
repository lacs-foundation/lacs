//! sysknife-daemon-test — integration test binary for the daemon IPC layer.
//!
//! Connects to a running sysknife-daemon socket, sends typed action messages,
//! and asserts on the responses. Exercises Tier 2 (daemon action execution) and
//! Tier 3 (IPC + approval gate) as described in docs/local/execution-validation-gap.md.
//!
//! Not installed on user machines. Built in the test VM and run via
//! `tests/e2e/atomic-vm.sh test-daemon`. Requires the caller to be a member of
//! the `sysknife` socket-gating group and the `sysknife-dev` role group.
//!
//! Output: TAP (https://testanything.org) to stdout.
//! Exit code: 0 if all tests pass, 1 if any fail, 2 on connection error.
//!
//! Environment:
//!   SYSKNIFE_LISTEN_URI  — socket URI (default: unix:///run/sysknife/daemon.sock)
//!   SYSKNIFE_TEST_USER   — username for authorized_keys tests (default: lacsdev)

use std::process;

use serde_json::{json, Value};
use sysknife_daemon::transport::framing::FramedStream;
use tokio::net::UnixStream;

/// Test-only SSH public key. Never used for real system access.
const TEST_SSH_KEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHgdnLwqhGo4FmOiMqhcvKDAeMJsKqdHTKxGdaemoTest sysknife-daemon-test@ci";

// ---------------------------------------------------------------------------
// TAP reporter
// ---------------------------------------------------------------------------

struct Tap {
    n: u32,
    failures: u32,
}

impl Tap {
    fn new() -> Self {
        Self { n: 1, failures: 0 }
    }

    fn ok(&mut self, desc: &str) {
        println!("ok {} - {desc}", self.n);
        self.n += 1;
    }

    fn fail(&mut self, desc: &str, reason: &str) {
        println!("not ok {} - {desc} # {reason}", self.n);
        self.n += 1;
        self.failures += 1;
    }

    fn finish(self) -> i32 {
        let total = self.n - 1;
        println!("1..{total}");
        if self.failures > 0 {
            1
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// IPC helpers
// ---------------------------------------------------------------------------

async fn send_recv(framed: &mut FramedStream<UnixStream>, req: Value) -> Value {
    framed
        .send(&serde_json::to_vec(&req).unwrap())
        .await
        .expect("framed send failed");
    let raw = framed.recv().await.expect("framed recv failed");
    serde_json::from_slice(&raw).expect("daemon returned invalid JSON")
}

/// Drain messages until `job_completed` arrives and return it.
/// Panics after 50 frames to prevent an infinite loop on a misbehaving daemon.
async fn drain_to_completed(framed: &mut FramedStream<UnixStream>) -> Value {
    for _ in 0..50 {
        let raw = framed.recv().await.expect("framed recv failed");
        let msg: Value = serde_json::from_slice(&raw).expect("daemon returned invalid JSON");
        if msg["type"] == "job_completed" {
            return msg;
        }
    }
    panic!("job_completed not received within 50 frames");
}

// ---------------------------------------------------------------------------
// Test suites
// ---------------------------------------------------------------------------

/// T1–T5: query_action read-only requests (Observer-level).
///
/// Verifies that the IPC transport delivers query_action messages and the daemon
/// returns properly typed responses. Also pins the informational-exit whitelist
/// (`GetServiceStatus` with a nonexistent unit exits 4, which must pass through).
async fn run_query_action_tests(framed: &mut FramedStream<UnixStream>, tap: &mut Tap) {
    // T1: GetDiskUsage — df output expected
    let r = send_recv(
        framed,
        json!({
            "type": "query_action",
            "request_id": "t1",
            "action_name": "GetDiskUsage",
            "params": {}
        }),
    )
    .await;
    if r["type"] == "query_action_response" && !r["output"].as_str().unwrap_or("").is_empty() {
        tap.ok("query_action GetDiskUsage returns non-empty output");
    } else {
        tap.fail(
            "query_action GetDiskUsage returns non-empty output",
            &format!("got: {r}"),
        );
    }

    // T2: GetMemoryInfo — free output expected
    let r = send_recv(
        framed,
        json!({
            "type": "query_action",
            "request_id": "t2",
            "action_name": "GetMemoryInfo",
            "params": {}
        }),
    )
    .await;
    if r["type"] == "query_action_response" && !r["output"].as_str().unwrap_or("").is_empty() {
        tap.ok("query_action GetMemoryInfo returns non-empty output");
    } else {
        tap.fail(
            "query_action GetMemoryInfo returns non-empty output",
            &format!("got: {r}"),
        );
    }

    // T3: GetServiceStatus for the running daemon — response is query_action_response
    // (exit 0 = active, or exit 1–4 = still informational)
    let r = send_recv(
        framed,
        json!({
            "type": "query_action",
            "request_id": "t3",
            "action_name": "GetServiceStatus",
            "params": { "unit": "sysknife-daemon.service" }
        }),
    )
    .await;
    if r["type"] == "query_action_response" {
        tap.ok("query_action GetServiceStatus running unit returns query_action_response");
    } else {
        tap.fail(
            "query_action GetServiceStatus running unit returns query_action_response",
            &format!("got: {r}"),
        );
    }

    // T4: GetServiceStatus for a nonexistent unit — exit 4 must be informational (not error)
    let r = send_recv(
        framed,
        json!({
            "type": "query_action",
            "request_id": "t4",
            "action_name": "GetServiceStatus",
            "params": { "unit": "sysknife-nonexistent-test-unit.service" }
        }),
    )
    .await;
    if r["type"] == "query_action_response" {
        tap.ok("query_action GetServiceStatus nonexistent unit (exit 4) is informational");
    } else {
        tap.fail(
            "query_action GetServiceStatus nonexistent unit (exit 4) is informational",
            &format!("got type={}", r["type"]),
        );
    }

    // T5: GetAuthorizedKeys for the test user
    let test_user =
        std::env::var("SYSKNIFE_TEST_USER").unwrap_or_else(|_| "lacsdev".to_string());
    let r = send_recv(
        framed,
        json!({
            "type": "query_action",
            "request_id": "t5",
            "action_name": "GetAuthorizedKeys",
            "params": { "username": test_user }
        }),
    )
    .await;
    if r["type"] == "query_action_response" {
        tap.ok("query_action GetAuthorizedKeys returns query_action_response");
    } else {
        tap.fail(
            "query_action GetAuthorizedKeys returns query_action_response",
            &format!("got: {r}"),
        );
    }
}

/// T6: preview returns a preview_response with a non-empty request_hash.
///
/// Verifies the preview handler builds an ActionSpec, runs the state query,
/// and returns a signed envelope the shell can display to the user.
async fn run_preview_test(framed: &mut FramedStream<UnixStream>, tap: &mut Tap) {
    let r = send_recv(
        framed,
        json!({
            "type": "preview",
            "request_id": "t6",
            "action_name": "GetDiskUsage",
            "params": {}
        }),
    )
    .await;
    let hash = r["preview"]["request_hash"].as_str().unwrap_or("").to_string();
    if r["type"] == "preview_response" && !hash.is_empty() {
        tap.ok("preview GetDiskUsage returns preview_response with non-empty request_hash");
    } else {
        tap.fail(
            "preview GetDiskUsage returns preview_response with non-empty request_hash",
            &format!("got: {r}"),
        );
    }
}

/// T7–T13: AddAuthorizedKey / RemoveAuthorizedKey full execute cycle.
///
/// Exercises the complete Tier 2 + Tier 3 path:
///   preview → execute → job_started → job_completed → system state assertion
///
/// Requires the caller to be in the sysknife-dev group.
/// The test key is removed at the end — the cycle is self-cleaning.
async fn run_ssh_key_cycle(
    framed: &mut FramedStream<UnixStream>,
    tap: &mut Tap,
    test_user: &str,
) {
    let keys_path = format!("/home/{test_user}/.ssh/authorized_keys");

    // T7: preview AddAuthorizedKey
    let r = send_recv(
        framed,
        json!({
            "type": "preview",
            "request_id": "t7",
            "action_name": "AddAuthorizedKey",
            "params": { "username": test_user, "public_key": TEST_SSH_KEY }
        }),
    )
    .await;
    let add_hash = r["preview"]["request_hash"].as_str().unwrap_or("").to_string();
    if r["type"] == "preview_response" && !add_hash.is_empty() {
        tap.ok("preview AddAuthorizedKey returns preview_response");
    } else {
        tap.fail(
            "preview AddAuthorizedKey returns preview_response",
            &format!("got: {r}"),
        );
        // Cannot execute without a valid hash — skip remaining SSH tests.
        for _ in 7..=13 {
            tap.fail(
                "SSH key cycle skipped",
                "preview AddAuthorizedKey failed — check sysknife-dev group membership",
            );
        }
        return;
    }

    // T8: execute AddAuthorizedKey — expect job_started first
    let r = send_recv(
        framed,
        json!({
            "type": "execute",
            "request_id": "t8",
            "action_name": "AddAuthorizedKey",
            "params": { "username": test_user, "public_key": TEST_SSH_KEY },
            "approval_hash": add_hash
        }),
    )
    .await;
    if r["type"] == "job_started" {
        tap.ok("execute AddAuthorizedKey returns job_started");
    } else {
        tap.fail(
            "execute AddAuthorizedKey returns job_started",
            &format!("got: {r}"),
        );
    }

    // T9: job_completed for AddAuthorizedKey
    let completed = drain_to_completed(framed).await;
    let status = completed["result"]["status"].as_str().unwrap_or("");
    if status == "success" {
        tap.ok("AddAuthorizedKey job_completed with success");
    } else {
        tap.fail(
            "AddAuthorizedKey job_completed with success",
            &format!("status={status:?}; completed={completed}"),
        );
    }

    // T10: Verify key is present in authorized_keys
    match std::fs::read_to_string(&keys_path) {
        Ok(content) if content.contains(TEST_SSH_KEY) => {
            tap.ok("AddAuthorizedKey: test key present in authorized_keys");
        }
        Ok(content) => {
            tap.fail(
                "AddAuthorizedKey: test key present in authorized_keys",
                &format!("key not found; file: {content:?}"),
            );
        }
        Err(e) => {
            tap.fail(
                "AddAuthorizedKey: test key present in authorized_keys",
                &format!("cannot read {keys_path}: {e}"),
            );
        }
    }

    // T11: preview RemoveAuthorizedKey
    let r = send_recv(
        framed,
        json!({
            "type": "preview",
            "request_id": "t11",
            "action_name": "RemoveAuthorizedKey",
            "params": { "username": test_user, "public_key": TEST_SSH_KEY }
        }),
    )
    .await;
    let remove_hash = r["preview"]["request_hash"].as_str().unwrap_or("").to_string();
    if r["type"] == "preview_response" && !remove_hash.is_empty() {
        tap.ok("preview RemoveAuthorizedKey returns preview_response");
    } else {
        tap.fail(
            "preview RemoveAuthorizedKey returns preview_response",
            &format!("got: {r}"),
        );
        return;
    }

    // T12: execute RemoveAuthorizedKey — expect job_started first
    let r = send_recv(
        framed,
        json!({
            "type": "execute",
            "request_id": "t12",
            "action_name": "RemoveAuthorizedKey",
            "params": { "username": test_user, "public_key": TEST_SSH_KEY },
            "approval_hash": remove_hash
        }),
    )
    .await;
    if r["type"] == "job_started" {
        tap.ok("execute RemoveAuthorizedKey returns job_started");
    } else {
        tap.fail(
            "execute RemoveAuthorizedKey returns job_started",
            &format!("got: {r}"),
        );
    }

    let completed = drain_to_completed(framed).await;
    let status = completed["result"]["status"].as_str().unwrap_or("");
    if status == "success" {
        tap.ok("RemoveAuthorizedKey job_completed with success");
    } else {
        tap.fail(
            "RemoveAuthorizedKey job_completed with success",
            &format!("status={status:?}"),
        );
    }

    // T13: Verify key is gone from authorized_keys
    match std::fs::read_to_string(&keys_path) {
        Ok(content) if !content.contains(TEST_SSH_KEY) => {
            tap.ok("RemoveAuthorizedKey: test key absent from authorized_keys");
        }
        Ok(_) => {
            tap.fail(
                "RemoveAuthorizedKey: test key absent from authorized_keys",
                "key still present after remove",
            );
        }
        Err(e) => {
            tap.fail(
                "RemoveAuthorizedKey: test key absent from authorized_keys",
                &format!("cannot read {keys_path}: {e}"),
            );
        }
    }
}

/// T14: execute without a prior preview returns stale_approval.
///
/// Verifies the "no prior preview" guard in handle_execute. The approval
/// gate must reject an execute whose hash was never produced by a preview.
async fn run_stale_approval_test(framed: &mut FramedStream<UnixStream>, tap: &mut Tap) {
    let r = send_recv(
        framed,
        json!({
            "type": "execute",
            "request_id": "t14",
            "action_name": "GetDiskUsage",
            "params": {},
            "approval_hash": "this-hash-was-never-issued-by-a-preview"
        }),
    )
    .await;
    if r["type"] == "error_response" && r["category"] == "stale_approval" {
        tap.ok("execute without prior preview returns stale_approval error");
    } else {
        tap.fail(
            "execute without prior preview returns stale_approval error",
            &format!("got: {r}"),
        );
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let uri = std::env::var("SYSKNIFE_LISTEN_URI")
        .unwrap_or_else(|_| "unix:///run/sysknife/daemon.sock".to_string());
    let socket_path = uri.strip_prefix("unix://").unwrap_or(&uri);

    let stream = match UnixStream::connect(socket_path).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("FATAL: cannot connect to {socket_path}: {e}");
            eprintln!("  Is sysknife-daemon running?");
            eprintln!("  Is this user in the 'sysknife' group?");
            process::exit(2);
        }
    };

    let mut framed = FramedStream::new(stream);
    let mut tap = Tap::new();
    let test_user =
        std::env::var("SYSKNIFE_TEST_USER").unwrap_or_else(|_| "lacsdev".to_string());

    run_query_action_tests(&mut framed, &mut tap).await;
    run_preview_test(&mut framed, &mut tap).await;
    run_ssh_key_cycle(&mut framed, &mut tap, &test_user).await;
    run_stale_approval_test(&mut framed, &mut tap).await;

    process::exit(tap.finish());
}
