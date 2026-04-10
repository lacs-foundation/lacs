use crate::state_client::{CuratedState, StateClient};
use lacs_types::RiskLevel;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
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

/// A single action within a plan.
///
/// Fields are private to enforce the invariant that `approval_required` is always
/// `true` for `Medium` and `High` risk steps and `false` for `Low` risk steps.
/// Use [`read_only_step`] or [`mutating_step`] (module-private) to construct.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    action_name: String,
    summary: String,
    risk_level: PlanRiskLevel,
    approval_required: bool,
}

impl PlanStep {
    pub fn action_name(&self) -> &str {
        &self.action_name
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn risk_level(&self) -> &PlanRiskLevel {
        &self.risk_level
    }

    pub fn approval_required(&self) -> bool {
        self.approval_required
    }
}

/// A complete plan produced by the planner for a given intent.
///
/// Fields are private to enforce construction through [`Planner::plan_intent`],
/// which validates the intent and guarantees `steps` is non-empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    intent: String,
    current_state: CuratedState,
    summary: String,
    steps: Vec<PlanStep>,
}

impl Plan {
    pub fn intent(&self) -> &str {
        &self.intent
    }

    pub fn current_state(&self) -> &CuratedState {
        &self.current_state
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn steps(&self) -> &[PlanStep] {
        &self.steps
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PlanningError {
    #[error("intent must not be empty")]
    EmptyIntent,

    #[error("state unavailable: {0}")]
    StateUnavailable(String),
}

/// Plans user intents into typed, approval-gated action sequences.
///
/// Generic over `C: StateClient` so tests can inject a mock without I/O.
/// Uses `AtomicUsize` internally so `Planner<C>` is `Sync` when `C: Sync`,
/// enabling use as Tauri managed state.
pub struct Planner<C> {
    state_client: C,
    call_count: AtomicUsize,
}

impl<C> Planner<C>
where
    C: StateClient,
{
    pub fn new(state_client: C) -> Self {
        Self {
            state_client,
            call_count: AtomicUsize::new(0),
        }
    }

    pub fn plan_intent(&self, intent: &str) -> Result<Plan, PlanningError> {
        let intent = intent.trim();
        if intent.is_empty() {
            return Err(PlanningError::EmptyIntent);
        }

        self.call_count.fetch_add(1, Ordering::Relaxed);
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
