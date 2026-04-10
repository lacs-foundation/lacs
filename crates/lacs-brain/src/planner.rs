use crate::state_client::{CuratedState, StateClient};
use lacs_types::RiskLevel;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanRiskLevel {
    Low,
    Medium,
    High,
}

impl PlanRiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl From<RiskLevel> for PlanRiskLevel {
    fn from(value: RiskLevel) -> Self {
        match value {
            RiskLevel::Low => Self::Low,
            RiskLevel::Medium => Self::Medium,
            RiskLevel::High => Self::High,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub action_name: String,
    pub summary: String,
    pub risk_level: PlanRiskLevel,
    pub approval_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub intent: String,
    pub current_state: CuratedState,
    pub summary: String,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PlanningError {
    #[error("intent must not be empty")]
    EmptyIntent,

    #[error("state unavailable: {0}")]
    StateUnavailable(String),
}

pub struct Planner<C> {
    state_client: C,
    call_count: Cell<usize>,
}

impl<C> Planner<C>
where
    C: StateClient,
{
    pub fn new(state_client: C) -> Self {
        Self {
            state_client,
            call_count: Cell::new(0),
        }
    }

    pub fn state_client_call_count(&self) -> usize {
        self.call_count.get()
    }

    pub fn plan_intent(&self, intent: &str) -> Result<Plan, PlanningError> {
        let intent = intent.trim();
        if intent.is_empty() {
            return Err(PlanningError::EmptyIntent);
        }

        self.call_count.set(self.call_count.get() + 1);
        let current_state = self.state_client.curated_state()?;
        let lowered = intent.to_ascii_lowercase();
        let mut steps = Vec::new();

        if lowered.contains("show") || lowered.contains("state") {
            steps.push(read_only_step(
                "GetSystemState",
                "show curated system state",
            ));
        }

        if lowered.contains("update") {
            steps.push(mutating_step(
                "UpdateSystem",
                "stage an updated ostree deployment",
                PlanRiskLevel::High,
            ));
        }

        if lowered.contains("toolbox") && lowered.contains("create") {
            steps.push(mutating_step(
                "CreateToolbox",
                "create a development toolbox",
                PlanRiskLevel::Medium,
            ));
        }

        if lowered.contains("install") && lowered.contains("firefox") {
            steps.push(mutating_step(
                "InstallFlatpak",
                "install firefox from the approved remote",
                PlanRiskLevel::Medium,
            ));
        }

        if steps.is_empty() {
            steps.push(read_only_step(
                "GetSystemState",
                "inspect the curated system state before acting",
            ));
        }

        let requires_approval = steps.iter().any(|step| step.approval_required);
        let summary = if requires_approval {
            format!(
                "Plan for `{intent}` requires approval on {}",
                current_state.host_name
            )
        } else {
            format!(
                "Plan for `{intent}` is read-only on {}",
                current_state.host_name
            )
        };

        Ok(Plan {
            intent: intent.to_string(),
            current_state,
            summary,
            steps,
        })
    }
}

fn read_only_step(action_name: &str, summary: &str) -> PlanStep {
    PlanStep {
        action_name: action_name.to_string(),
        summary: summary.to_string(),
        risk_level: PlanRiskLevel::Low,
        approval_required: false,
    }
}

fn mutating_step(action_name: &str, summary: &str, risk_level: PlanRiskLevel) -> PlanStep {
    PlanStep {
        action_name: action_name.to_string(),
        summary: summary.to_string(),
        risk_level,
        approval_required: true,
    }
}
