use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerRole {
    Observer,
    Dev,
    Admin,
    Boot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    RolledBack,
    NeedsReboot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    ValidationFailure,
    AuthorizationFailure,
    PolicyDenied,
    StaleApproval,
    ExecutionFailure,
    TransientInfrastructureFailure,
    Cancellation,
    StuckExecution,
    RebootRequired,
    RollbackFailure,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub action_name: String,
    pub request_id: String,
    pub params: Value,
    pub caller_role: CallerRole,
    pub request_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreviewEnvelope {
    pub summary: String,
    pub risk_level: RiskLevel,
    pub current_state: Value,
    pub proposed_change: Value,
    pub expected_side_effects: Vec<String>,
    pub reboot_required: bool,
    pub rollback_available: bool,
    pub warnings: Vec<String>,
    pub request_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultEnvelope {
    pub status: JobState,
    pub summary: String,
    pub warnings: Vec<String>,
    pub job_id: Option<String>,
    pub needs_reboot: bool,
    pub rollback_ref: Option<String>,
    pub transaction_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub transaction_id: String,
    pub request_id: String,
    pub request_hash: String,
    pub action_name: String,
    pub risk_level: RiskLevel,
    pub status: JobState,
    pub approval_id: Option<String>,
    pub summary: String,
    pub warnings: Vec<String>,
}
