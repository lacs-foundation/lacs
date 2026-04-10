use lacs_daemon::auth::highest_role_from_groups;
use lacs_daemon::jobs::JobStateMachine;
use lacs_daemon::policy::approval_matches_request;
use lacs_daemon::transactions::{NewTransaction, TransactionStore};
use lacs_daemon::transport::grpc::{bind_unix_listener, ListenTarget};
use lacs_types::{CallerRole, JobState, RiskLevel};
use tempfile::tempdir;

#[test]
fn unix_socket_startup_rejects_tcp_uris() {
    let unix = ListenTarget::try_from_uri("unix:///tmp/lacs.sock").expect("unix uri should parse");

    assert!(matches!(unix, ListenTarget::Unix(_)));
    assert!(ListenTarget::try_from_uri("tcp://127.0.0.1:7000").is_err());
}

#[test]
fn unix_socket_listener_is_created_on_disk() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("lacs.sock");
    let target = ListenTarget::try_from_uri(&format!("unix://{}", socket_path.display()))
        .expect("unix uri should parse");

    let listener = bind_unix_listener(&target).expect("bind unix listener");
    assert!(socket_path.exists());
    drop(listener);
}

#[test]
fn highest_role_is_derived_from_local_groups() {
    assert_eq!(highest_role_from_groups(["lacs-dev"]), CallerRole::Dev);
    assert_eq!(
        highest_role_from_groups(["wheel", "lacs-dev"]),
        CallerRole::Admin
    );
    assert_eq!(
        highest_role_from_groups(["lacs-boot", "wheel"]),
        CallerRole::Boot
    );
    assert_eq!(
        highest_role_from_groups(std::iter::empty::<&str>()),
        CallerRole::Observer
    );
}

#[test]
fn approval_hashes_must_match_request_hash() {
    assert!(approval_matches_request("req-123", "req-123"));
    assert!(!approval_matches_request("req-123", "req-456"));
}

#[test]
fn transaction_records_are_persisted() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("lacs.sqlite");
    let store = TransactionStore::open(&db_path).expect("open store");

    let transaction = NewTransaction {
        request_id: "request-1".into(),
        request_hash: "hash-1".into(),
        action_name: "UpdateSystem".into(),
        risk_level: RiskLevel::High,
        status: JobState::Queued,
        approval_id: Some("approval-1".into()),
        summary: "Stage system update".into(),
        warnings: vec!["reboot required".into()],
    };

    let record = store.record(transaction).expect("record tx");
    let loaded = store
        .get(&record.transaction_id)
        .expect("load tx")
        .expect("transaction exists");

    assert_eq!(loaded.request_id, "request-1");
    assert_eq!(loaded.request_hash, "hash-1");
    assert_eq!(loaded.action_name, "UpdateSystem");
    assert_eq!(loaded.status, JobState::Queued);
    assert_eq!(loaded.approval_id.as_deref(), Some("approval-1"));
    assert_eq!(loaded.warnings, vec!["reboot required".to_string()]);
}

#[test]
fn job_state_machine_rejects_invalid_transitions() {
    let mut job = JobStateMachine::new("job-1");

    assert_eq!(job.state(), JobState::Queued);
    job.transition_to(JobState::Running)
        .expect("queued -> running");
    job.transition_to(JobState::NeedsReboot)
        .expect("running -> needs reboot");
    assert_eq!(job.state(), JobState::NeedsReboot);
    assert!(job.transition_to(JobState::Running).is_err());
}
