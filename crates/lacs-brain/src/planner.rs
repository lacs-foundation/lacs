//! Core planning types and `LlmPlanner`.
//!
//! `LlmPlanner` drives a tool-use loop with a configured `LlmProvider`,
//! calls `StateClient::curated_state()` when the LLM invokes the
//! `get_system_state` tool, and returns a validated `Plan` when the LLM
//! calls `propose_plan`.
//!
//! The loop is bounded by `max_turns`. If the LLM exhausts all turns without
//! calling `propose_plan`, the planner returns `PlanningError::PlannerStuck`.
//!
//! Note: `StateClient` is invoked synchronously (blocking the async task) on
//! each `get_system_state` call. If the state client ever needs I/O, the trait
//! must be made async.

use crate::config::{BrainConfig, ProviderConfig};
use crate::planning_tools::get_state::get_state_tool_def;
use crate::planning_tools::propose_plan::{parse_proposed_plan, propose_plan_tool_def};
use crate::prompt::build_system_prompt;
use crate::provider::{
    ContentBlock, LlmProvider, Message, ProviderError, Role, StopReason, ToolDefinition,
    ToolResultBlock,
};
use crate::providers::anthropic::AnthropicProvider;
use crate::providers::ollama::OllamaProvider;
use crate::state_client::StateClient;
use serde::Serialize;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Risk level
// ---------------------------------------------------------------------------

/// Risk classification for a single plan step.
///
/// Determines whether the step requires explicit user approval before execution.
/// Serialises to lowercase strings (`"low"`, `"medium"`, `"high"`) matching the
/// values expected by `parse_proposed_plan` and the system prompt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
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
/// `approval_required` is a pure function of `risk_level`: `Low` → false,
/// `Medium`/`High` → true. It is not stored separately to prevent the class of
/// bugs where the stored value disagrees with the risk level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlanStep {
    action_name: String,
    summary: String,
    risk_level: PlanRiskLevel,
    params: serde_json::Value,
}

impl PlanStep {
    pub fn new(
        action_name: String,
        summary: String,
        risk_level: PlanRiskLevel,
        params: serde_json::Value,
    ) -> Self {
        Self {
            action_name,
            summary,
            risk_level,
            params,
        }
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

    /// Derived from risk level: `true` for Medium and High, `false` for Low.
    pub fn approval_required(&self) -> bool {
        !matches!(self.risk_level, PlanRiskLevel::Low)
    }

    pub fn params(&self) -> &serde_json::Value {
        &self.params
    }
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

/// A complete, validated plan returned by `LlmPlanner::plan_intent`.
///
/// Guaranteed to have at least one step. Constructed only through
/// `parse_proposed_plan`, which validates all fields before calling `Plan::new`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Plan {
    intent: String,
    summary: String,
    explanation: String,
    steps: Vec<PlanStep>,
}

impl Plan {
    /// Construct a plan. Panics if `steps` is empty or any string field is
    /// empty — these are programmer errors; callers must validate first.
    pub fn new(intent: String, summary: String, explanation: String, steps: Vec<PlanStep>) -> Self {
        assert!(!intent.is_empty(), "Plan intent must not be empty");
        assert!(!summary.is_empty(), "Plan summary must not be empty");
        assert!(
            !explanation.is_empty(),
            "Plan explanation must not be empty"
        );
        assert!(!steps.is_empty(), "Plan must have at least one step");
        Self {
            intent,
            summary,
            explanation,
            steps,
        }
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

/// Drives the LLM planning loop.
///
/// The system prompt and tool definitions are built once at construction time
/// and reused across all planning calls on this instance.
pub struct LlmPlanner {
    provider: Box<dyn LlmProvider>,
    state_client: Box<dyn StateClient>,
    max_turns: usize,
    system_prompt: String,
    tools: Vec<ToolDefinition>,
}

impl LlmPlanner {
    /// Construct a planner directly.
    ///
    /// # Panics
    /// Panics if `max_turns` is zero.
    pub fn new(
        provider: Box<dyn LlmProvider>,
        state_client: Box<dyn StateClient>,
        max_turns: usize,
    ) -> Self {
        assert!(max_turns >= 1, "max_turns must be at least 1");
        Self {
            provider,
            state_client,
            max_turns,
            system_prompt: build_system_prompt(),
            tools: vec![get_state_tool_def(), propose_plan_tool_def()],
        }
    }

    /// Construct a planner from a [`BrainConfig`].
    ///
    /// Returns an error if the HTTP client cannot be initialised (rare; only
    /// fails if the TLS subsystem is unavailable).
    pub fn from_config(
        config: BrainConfig,
        state_client: Box<dyn StateClient>,
    ) -> Result<Self, String> {
        let provider: Box<dyn LlmProvider> = match config.provider {
            ProviderConfig::Anthropic {
                api_key,
                model,
                base_url,
            } => Box::new(
                AnthropicProvider::new(&api_key, &model, &base_url).map_err(|e| e.to_string())?,
            ),
            ProviderConfig::Ollama { base_url, model } => {
                Box::new(OllamaProvider::new(&base_url, &model).map_err(|e| e.to_string())?)
            }
        };
        Ok(Self::new(provider, state_client, config.max_turns))
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

        let mut messages: Vec<Message> = vec![Message::user_text(intent)];

        for _turn in 0..self.max_turns {
            let completion = self
                .provider
                .complete(&self.system_prompt, &messages, &self.tools, 4096)
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

                    for (id, name, input) in &tool_calls {
                        match name.as_str() {
                            "get_system_state" => {
                                let state = self.state_client.curated_state()?;
                                let state_json =
                                    serde_json::to_string(&state).unwrap_or_else(|_| "{}".into());
                                tool_results.push(ToolResultBlock {
                                    tool_use_id: id.clone(),
                                    content: state_json,
                                    is_error: false,
                                });
                            }
                            "propose_plan" => {
                                // Parse and validate before returning — if parse fails
                                // the error propagates via `?` without adding a tool result.
                                let plan = parse_proposed_plan(intent, input)?;
                                return Ok(plan);
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

                    messages.push(Message::tool_results(tool_results));
                }
            }
        }

        Err(PlanningError::PlannerStuck)
    }
}
