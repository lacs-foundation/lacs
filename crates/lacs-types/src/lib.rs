use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::TryFrom;

use lacs_proto::lacs::v1 as proto;

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

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("invalid json payload for {0}")]
    InvalidJson(&'static str, #[source] serde_json::Error),

    #[error("invalid enum value {value} for {field}")]
    InvalidEnum { field: &'static str, value: i32 },
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

fn caller_role_code(value: CallerRole) -> i32 {
    match value {
        CallerRole::Observer => 1,
        CallerRole::Dev => 2,
        CallerRole::Admin => 3,
        CallerRole::Boot => 4,
    }
}

fn risk_level_code(value: RiskLevel) -> i32 {
    match value {
        RiskLevel::Low => 1,
        RiskLevel::Medium => 2,
        RiskLevel::High => 3,
    }
}

fn job_state_code(value: JobState) -> i32 {
    match value {
        JobState::Queued => 1,
        JobState::Running => 2,
        JobState::Succeeded => 3,
        JobState::Failed => 4,
        JobState::Canceled => 5,
        JobState::RolledBack => 6,
        JobState::NeedsReboot => 7,
    }
}

fn failure_category_code(value: FailureCategory) -> i32 {
    match value {
        FailureCategory::ValidationFailure => 1,
        FailureCategory::AuthorizationFailure => 2,
        FailureCategory::PolicyDenied => 3,
        FailureCategory::StaleApproval => 4,
        FailureCategory::ExecutionFailure => 5,
        FailureCategory::TransientInfrastructureFailure => 6,
        FailureCategory::Cancellation => 7,
        FailureCategory::StuckExecution => 8,
        FailureCategory::RebootRequired => 9,
        FailureCategory::RollbackFailure => 10,
    }
}

impl From<CallerRole> for proto::CallerRole {
    fn from(value: CallerRole) -> Self {
        proto::CallerRole::try_from(caller_role_code(value)).expect("valid caller role")
    }
}

impl TryFrom<proto::CallerRole> for CallerRole {
    type Error = BridgeError;

    fn try_from(value: proto::CallerRole) -> Result<Self, Self::Error> {
        match i32::from(value) {
            1 => Ok(CallerRole::Observer),
            2 => Ok(CallerRole::Dev),
            3 => Ok(CallerRole::Admin),
            4 => Ok(CallerRole::Boot),
            code => Err(BridgeError::InvalidEnum {
                field: "caller_role",
                value: code,
            }),
        }
    }
}

impl From<RiskLevel> for proto::RiskLevel {
    fn from(value: RiskLevel) -> Self {
        proto::RiskLevel::try_from(risk_level_code(value)).expect("valid risk level")
    }
}

impl TryFrom<proto::RiskLevel> for RiskLevel {
    type Error = BridgeError;

    fn try_from(value: proto::RiskLevel) -> Result<Self, Self::Error> {
        match i32::from(value) {
            1 => Ok(RiskLevel::Low),
            2 => Ok(RiskLevel::Medium),
            3 => Ok(RiskLevel::High),
            code => Err(BridgeError::InvalidEnum {
                field: "risk_level",
                value: code,
            }),
        }
    }
}

impl From<JobState> for proto::JobState {
    fn from(value: JobState) -> Self {
        proto::JobState::try_from(job_state_code(value)).expect("valid job state")
    }
}

impl TryFrom<proto::JobState> for JobState {
    type Error = BridgeError;

    fn try_from(value: proto::JobState) -> Result<Self, Self::Error> {
        match i32::from(value) {
            1 => Ok(JobState::Queued),
            2 => Ok(JobState::Running),
            3 => Ok(JobState::Succeeded),
            4 => Ok(JobState::Failed),
            5 => Ok(JobState::Canceled),
            6 => Ok(JobState::RolledBack),
            7 => Ok(JobState::NeedsReboot),
            code => Err(BridgeError::InvalidEnum {
                field: "job_state",
                value: code,
            }),
        }
    }
}

impl From<FailureCategory> for proto::FailureCategory {
    fn from(value: FailureCategory) -> Self {
        proto::FailureCategory::try_from(failure_category_code(value))
            .expect("valid failure category")
    }
}

impl TryFrom<proto::FailureCategory> for FailureCategory {
    type Error = BridgeError;

    fn try_from(value: proto::FailureCategory) -> Result<Self, Self::Error> {
        match i32::from(value) {
            1 => Ok(FailureCategory::ValidationFailure),
            2 => Ok(FailureCategory::AuthorizationFailure),
            3 => Ok(FailureCategory::PolicyDenied),
            4 => Ok(FailureCategory::StaleApproval),
            5 => Ok(FailureCategory::ExecutionFailure),
            6 => Ok(FailureCategory::TransientInfrastructureFailure),
            7 => Ok(FailureCategory::Cancellation),
            8 => Ok(FailureCategory::StuckExecution),
            9 => Ok(FailureCategory::RebootRequired),
            10 => Ok(FailureCategory::RollbackFailure),
            code => Err(BridgeError::InvalidEnum {
                field: "failure_category",
                value: code,
            }),
        }
    }
}

impl From<RequestEnvelope> for proto::RequestEnvelope {
    fn from(value: RequestEnvelope) -> Self {
        Self {
            action_name: value.action_name,
            request_id: value.request_id,
            params_json: serde_json::to_string(&value.params).expect("json serialization"),
            caller_role: caller_role_code(value.caller_role),
            request_hash: value.request_hash,
        }
    }
}

impl TryFrom<proto::RequestEnvelope> for RequestEnvelope {
    type Error = BridgeError;

    fn try_from(value: proto::RequestEnvelope) -> Result<Self, Self::Error> {
        let params = serde_json::from_str(&value.params_json)
            .map_err(|error| BridgeError::InvalidJson("params_json", error))?;

        Ok(Self {
            action_name: value.action_name,
            request_id: value.request_id,
            params,
            caller_role: CallerRole::try_from(
                proto::CallerRole::try_from(value.caller_role).map_err(|_| {
                    BridgeError::InvalidEnum {
                        field: "caller_role",
                        value: value.caller_role,
                    }
                })?,
            )?,
            request_hash: value.request_hash,
        })
    }
}

impl From<PreviewEnvelope> for proto::PreviewEnvelope {
    fn from(value: PreviewEnvelope) -> Self {
        Self {
            summary: value.summary,
            risk_level: risk_level_code(value.risk_level),
            current_state_json: serde_json::to_string(&value.current_state)
                .expect("json serialization"),
            proposed_change_json: serde_json::to_string(&value.proposed_change)
                .expect("json serialization"),
            expected_side_effects: value.expected_side_effects,
            reboot_required: value.reboot_required,
            rollback_available: value.rollback_available,
            warnings: value.warnings,
            request_hash: value.request_hash,
        }
    }
}

impl TryFrom<proto::PreviewEnvelope> for PreviewEnvelope {
    type Error = BridgeError;

    fn try_from(value: proto::PreviewEnvelope) -> Result<Self, Self::Error> {
        let current_state = serde_json::from_str(&value.current_state_json)
            .map_err(|error| BridgeError::InvalidJson("current_state_json", error))?;
        let proposed_change = serde_json::from_str(&value.proposed_change_json)
            .map_err(|error| BridgeError::InvalidJson("proposed_change_json", error))?;

        Ok(Self {
            summary: value.summary,
            risk_level: RiskLevel::try_from(
                proto::RiskLevel::try_from(value.risk_level).map_err(|_| {
                    BridgeError::InvalidEnum {
                        field: "risk_level",
                        value: value.risk_level,
                    }
                })?,
            )?,
            current_state,
            proposed_change,
            expected_side_effects: value.expected_side_effects,
            reboot_required: value.reboot_required,
            rollback_available: value.rollback_available,
            warnings: value.warnings,
            request_hash: value.request_hash,
        })
    }
}

impl From<ResultEnvelope> for proto::ResultEnvelope {
    fn from(value: ResultEnvelope) -> Self {
        Self {
            status: job_state_code(value.status),
            summary: value.summary,
            warnings: value.warnings,
            job_id: value.job_id.unwrap_or_default(),
            needs_reboot: value.needs_reboot,
            rollback_ref: value.rollback_ref.unwrap_or_default(),
            transaction_id: value.transaction_id,
        }
    }
}

impl TryFrom<proto::ResultEnvelope> for ResultEnvelope {
    type Error = BridgeError;

    fn try_from(value: proto::ResultEnvelope) -> Result<Self, Self::Error> {
        Ok(Self {
            status: JobState::try_from(proto::JobState::try_from(value.status).map_err(|_| {
                BridgeError::InvalidEnum {
                    field: "job_state",
                    value: value.status,
                }
            })?)?,
            summary: value.summary,
            warnings: value.warnings,
            job_id: if value.job_id.is_empty() {
                None
            } else {
                Some(value.job_id)
            },
            needs_reboot: value.needs_reboot,
            rollback_ref: if value.rollback_ref.is_empty() {
                None
            } else {
                Some(value.rollback_ref)
            },
            transaction_id: value.transaction_id,
        })
    }
}

impl From<TransactionRecord> for proto::TransactionRecord {
    fn from(value: TransactionRecord) -> Self {
        Self {
            transaction_id: value.transaction_id,
            request_id: value.request_id,
            request_hash: value.request_hash,
            action_name: value.action_name,
            risk_level: risk_level_code(value.risk_level),
            status: job_state_code(value.status),
            approval_id: value.approval_id.unwrap_or_default(),
            summary: value.summary,
            warnings: value.warnings,
        }
    }
}

impl TryFrom<proto::TransactionRecord> for TransactionRecord {
    type Error = BridgeError;

    fn try_from(value: proto::TransactionRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            transaction_id: value.transaction_id,
            request_id: value.request_id,
            request_hash: value.request_hash,
            action_name: value.action_name,
            risk_level: RiskLevel::try_from(
                proto::RiskLevel::try_from(value.risk_level).map_err(|_| {
                    BridgeError::InvalidEnum {
                        field: "risk_level",
                        value: value.risk_level,
                    }
                })?,
            )?,
            status: JobState::try_from(proto::JobState::try_from(value.status).map_err(|_| {
                BridgeError::InvalidEnum {
                    field: "job_state",
                    value: value.status,
                }
            })?)?,
            approval_id: if value.approval_id.is_empty() {
                None
            } else {
                Some(value.approval_id)
            },
            summary: value.summary,
            warnings: value.warnings,
        })
    }
}
