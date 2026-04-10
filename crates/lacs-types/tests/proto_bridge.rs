use std::convert::TryInto;

use lacs_proto::lacs::v1 as proto;
use lacs_types::{
    CallerRole, FailureCategory, JobState, PreviewEnvelope, RequestEnvelope, ResultEnvelope,
    RiskLevel, TransactionRecord,
};

#[test]
fn caller_role_round_trips_through_proto() {
    let proto_role = proto::CallerRole::try_from(3).unwrap();
    let local_role = CallerRole::try_from(proto_role).unwrap();

    assert_eq!(local_role, CallerRole::Admin);
}

#[test]
fn job_state_round_trips_through_proto() {
    let proto_state = proto::JobState::try_from(7).unwrap();
    let local_state = JobState::try_from(proto_state).unwrap();

    assert_eq!(local_state, JobState::NeedsReboot);
}

#[test]
fn failure_category_round_trips_through_proto() {
    let proto_category = proto::FailureCategory::try_from(10).unwrap();
    let local_category = FailureCategory::try_from(proto_category).unwrap();

    assert_eq!(local_category, FailureCategory::RollbackFailure);
}

#[test]
fn request_envelope_round_trips_through_proto() {
    let value = RequestEnvelope {
        action_name: "InstallFlatpak".to_string(),
        request_id: "req-1".to_string(),
        params: serde_json::json!({"app_id": "org.mozilla.firefox"}),
        caller_role: CallerRole::Dev,
        request_hash: "abc123".to_string(),
    };

    let proto_value: proto::RequestEnvelope = value.clone().try_into().unwrap();
    let decoded = RequestEnvelope::try_from(proto_value).unwrap();

    assert_eq!(decoded, value);
}

#[test]
fn preview_envelope_round_trips_through_proto() {
    let value = PreviewEnvelope {
        summary: "Install Firefox".to_string(),
        risk_level: RiskLevel::Medium,
        current_state: serde_json::json!({"flatpaks": []}),
        proposed_change: serde_json::json!({"flatpaks": ["org.mozilla.firefox"]}),
        expected_side_effects: vec!["downloads application metadata".to_string()],
        reboot_required: false,
        rollback_available: true,
        warnings: vec!["network required".to_string()],
        request_hash: "abc123".to_string(),
    };

    let proto_value: proto::PreviewEnvelope = value.clone().try_into().unwrap();
    let decoded = PreviewEnvelope::try_from(proto_value).unwrap();

    assert_eq!(decoded, value);
}

#[test]
fn result_envelope_round_trips_through_proto() {
    let value = ResultEnvelope {
        status: JobState::Succeeded,
        summary: "Installed".to_string(),
        warnings: vec!["restart recommended".to_string()],
        job_id: Some("job-7".to_string()),
        needs_reboot: false,
        rollback_ref: Some("ostree:fedora/41/x86_64/silverblue".to_string()),
        transaction_id: "tx-42".to_string(),
    };

    let proto_value: proto::ResultEnvelope = value.clone().try_into().unwrap();
    let decoded = ResultEnvelope::try_from(proto_value).unwrap();

    assert_eq!(decoded, value);
}

#[test]
fn transaction_record_round_trips_through_proto() {
    let value = TransactionRecord {
        transaction_id: "tx-42".to_string(),
        request_id: "req-1".to_string(),
        request_hash: "abc123".to_string(),
        action_name: "InstallFlatpak".to_string(),
        risk_level: RiskLevel::Medium,
        status: JobState::Succeeded,
        approval_id: Some("approval-9".to_string()),
        summary: "Installed".to_string(),
        warnings: vec!["restart recommended".to_string()],
    };

    let proto_value: proto::TransactionRecord = value.clone().try_into().unwrap();
    let decoded = TransactionRecord::try_from(proto_value).unwrap();

    assert_eq!(decoded, value);
}
