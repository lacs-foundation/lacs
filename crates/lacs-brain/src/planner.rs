//! Core planning types and `LlmPlanner`.
//!
//! `LlmPlanner` drives a tool-use loop with a configured `LlmProvider`,
//! dispatches the `get_system_state` planning tool to the `StateClient`,
//! and returns a validated `Plan` when the LLM calls `propose_plan`.
//!
//! The loop is bounded by `max_turns`. If the LLM exhausts all turns without
//! calling `propose_plan`, the planner returns `PlanningError::PlannerStuck`.

use crate::planning_tools::get_state::get_state_tool_def;
use crate::planning_tools::propose_plan::{parse_proposed_plan, propose_plan_tool_def};
use crate::prompt::build_system_prompt;
use crate::provider::{
    ContentBlock, LlmProvider, Message, ProviderError, Role, StopReason, ToolDefinition,
    ToolResultBlock,
};
use crate::state_client::StateClient;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Risk level
// ---------------------------------------------------------------------------

/// Risk classification for a single plan step.
///
/// Determines whether the step requires explicit user approval before execution.
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

// ---------------------------------------------------------------------------
// PlanStep
// ---------------------------------------------------------------------------

/// A single action within a plan.
///
/// `approval_required` is derived from `risk_level` at construction time:
/// `Low` → false, `Medium`/`High` → true. Consumers should rely on
/// `approval_required()` rather than the risk level directly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    action_name: String,
    summary: String,
    risk_level: PlanRiskLevel,
    approval_required: bool,
    params: serde_json::Value,
}

impl PlanStep {
    /// Construct a step. `approval_required` is derived from `risk_level`.
    pub fn new(
        action_name: String,
        summary: String,
        risk_level: PlanRiskLevel,
        params: serde_json::Value,
    ) -> Self {
        let approval_required = !matches!(risk_level, PlanRiskLevel::Low);
        Self { action_name, summary, risk_level, approval_required, params }
    }

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

    pub fn params(&self) -> &serde_json::Value {
        &self.params
    }
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

/// A complete, validated plan returned by `LlmPlanner::plan_intent`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    intent: String,
    summary: String,
    explanation: String,
    steps: Vec<PlanStep>,
}

impl Plan {
    pub fn new(
        intent: String,
        summary: String,
        explanation: String,
        steps: Vec<PlanStep>,
    ) -> Self {
        Self { intent, summary, explanation, steps }
    }

    pub fn intent(&self) -> &str {
        &self.intent
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn explanation(&self) -> &str {
        &self.explanation
    }

    pub fn steps(&self) -> &[PlanStep] {
        &self.steps
    }
}

// ---------------------------------------------------------------------------
// PlanningError
// ---------------------------------------------------------------------------

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PlanningError {
    #[error("intent must not be empty")]
    EmptyIntent,

    #[error("state unavailable: {0}")]
    StateUnavailable(String),

    #[error("planner did not propose a plan within the allowed turns")]
    PlannerStuck,

    #[error("planner ended without proposing a plan")]
    NoPlanProposed,

    #[error("provider error: {0}")]
    Provider(String),

    #[error("invalid plan output: {0}")]
    InvalidPlanOutput(String),
}

impl From<ProviderError> for PlanningError {
    fn from(e: ProviderError) -> Self {
        Self::Provider(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// LlmPlanner
// ---------------------------------------------------------------------------

// Built once; reused across all planning requests in the process lifetime.
static SYSTEM_PROMPT: OnceLock<String> = OnceLock::new();
static TOOLS: OnceLock<[ToolDefinition; 2]> = OnceLock::new();

/// Drives the LLM planning loop.
///
/// Calls `LlmProvider::complete` in a loop until the LLM calls `propose_plan`
/// (success) or the turn budget is exhausted (error). The `StateClient` is
/// invoked synchronously whenever the LLM calls `get_system_state`.
pub struct LlmPlanner {
    provider: Box<dyn LlmProvider>,
    state_client: Box<dyn StateClient>,
    max_turns: usize,
}

impl LlmPlanner {
    pub fn new(
        provider: Box<dyn LlmProvider>,
        state_client: Box<dyn StateClient>,
        max_turns: usize,
    ) -> Self {
        Self { provider, state_client, max_turns }
    }

    /// Run the planning loop for the given natural-language intent.
    ///
    /// Returns `Err(EmptyIntent)` immediately if the intent is blank.
    /// Returns `Err(PlannerStuck)` if `max_turns` elapse without a plan.
    /// Returns `Err(NoPlanProposed)` if the LLM ends the turn without a plan.
    pub async fn plan_intent(&self, intent: &str) -> Result<Plan, PlanningError> {
        let intent = intent.trim();
        if intent.is_empty() {
            return Err(PlanningError::EmptyIntent);
        }

        let system = SYSTEM_PROMPT.get_or_init(build_system_prompt);
        let tools = TOOLS.get_or_init(|| [get_state_tool_def(), propose_plan_tool_def()]);

        let mut messages: Vec<Message> = vec![Message::user_text(intent)];

        for _turn in 0..self.max_turns {
            let completion = self
                .provider
                .complete(system, &messages, tools, 4096)
                .await
                .map_err(PlanningError::from)?;

            messages.push(Message {
                role: Role::Assistant,
                content: completion.content.clone(),
            });

            match completion.stop_reason {
                StopReason::EndTurn | StopReason::MaxTokens => {
                    return Err(PlanningError::NoPlanProposed);
                }
                StopReason::ToolUse => {
                    let tool_calls: Vec<_> = completion
                        .content
                        .iter()
                        .filter_map(|b| {
                            if let ContentBlock::ToolUse { id, name, input } = b {
                                Some((id.clone(), name.clone(), input.clone()))
                            } else {
                                None
                            }
                        })
                        .collect();

                    if tool_calls.is_empty() {
                        return Err(PlanningError::NoPlanProposed);
                    }

                    let mut tool_results: Vec<ToolResultBlock> =
                        Vec::with_capacity(tool_calls.len());
                    let mut proposed_plan: Option<Plan> = None;

                    for (id, name, input) in &tool_calls {
                        match name.as_str() {
                            "get_system_state" => {
                                let state = self.state_client.curated_state()?;
                                let state_json = serde_json::to_string(&state)
                                    .unwrap_or_else(|_| "{}".into());
                                tool_results.push(ToolResultBlock {
                                    tool_use_id: id.clone(),
                                    content: state_json,
                                    is_error: false,
                                });
                            }
                            "propose_plan" => {
                                let plan = parse_proposed_plan(intent, input)?;
                                tool_results.push(ToolResultBlock {
                                    tool_use_id: id.clone(),
                                    content: "Plan accepted.".into(),
                                    is_error: false,
                                });
                                proposed_plan = Some(plan);
                            }
                            unknown => {
                                tool_results.push(ToolResultBlock {
                                    tool_use_id: id.clone(),
                                    content: format!("unknown tool: {unknown}"),
                                    is_error: true,
                                });
                            }
                        }
                    }

                    if let Some(plan) = proposed_plan {
                        return Ok(plan);
                    }

                    messages.push(Message::tool_results(tool_results));
                }
            }
        }

        Err(PlanningError::PlannerStuck)
    }
}
