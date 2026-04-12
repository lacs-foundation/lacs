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
//! Note: `StateClient::curated_state()` is synchronous. The production
//! `DaemonIpcClient` in `lacs-shell` uses a blocking `UnixStream`; Tauri
//! async commands run on a thread pool so blocking is acceptable there.
//! Other runtimes using `StateClient` on a single-threaded async executor
//! must use `spawn_blocking`.

use crate::action_name::ActionName;
use crate::audit::SafetyAuditLog;
use crate::config::{BrainConfig, ProviderConfig};
use crate::planning_tools::get_state::get_state_tool_def;
use crate::planning_tools::propose_plan::{parse_proposed_plan, propose_plan_tool_def};
use crate::prompt::build_system_prompt;
use crate::provider::{
    ContentBlock, LlmProvider, Message, ProviderError, Role, StopReason, ToolDefinition,
    ToolResultBlock,
};
use crate::providers::rig_adapter::RigCompletionAdapter;
use crate::state_client::StateClient;
use rig::client::CompletionClient;
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
    action_name: ActionName,
    summary: String,
    risk_level: PlanRiskLevel,
    params: serde_json::Value,
}

impl PlanStep {
    /// Construct a step. Returns an error if `summary` is empty.
    ///
    /// `action_name` is an [`ActionName`] which guarantees membership in
    /// the approved action catalogue at construction time.
    pub fn new(
        action_name: ActionName,
        summary: String,
        risk_level: PlanRiskLevel,
        params: serde_json::Value,
    ) -> Result<Self, PlanValidationError> {
        if summary.is_empty() {
            return Err(PlanValidationError(
                "PlanStep summary must not be empty".into(),
            ));
        }
        Ok(Self {
            action_name,
            summary,
            risk_level,
            params,
        })
    }

    pub fn action_name(&self) -> &str {
        self.action_name.as_str()
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
    /// Construct a plan. Returns an error if `steps` is empty or any string
    /// field is empty.
    pub fn new(
        intent: String,
        summary: String,
        explanation: String,
        steps: Vec<PlanStep>,
    ) -> Result<Self, PlanValidationError> {
        if intent.is_empty() {
            return Err(PlanValidationError("Plan intent must not be empty".into()));
        }
        if summary.is_empty() {
            return Err(PlanValidationError("Plan summary must not be empty".into()));
        }
        if explanation.is_empty() {
            return Err(PlanValidationError(
                "Plan explanation must not be empty".into(),
            ));
        }
        if steps.is_empty() {
            return Err(PlanValidationError(
                "Plan must have at least one step".into(),
            ));
        }
        Ok(Self {
            intent,
            summary,
            explanation,
            steps,
        })
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
// PlanValidationError
// ---------------------------------------------------------------------------

/// Returned when `Plan::new` or `PlanStep::new` receives invalid arguments.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{0}")]
pub struct PlanValidationError(pub String);

// ---------------------------------------------------------------------------
// PlanningError
// ---------------------------------------------------------------------------

#[non_exhaustive]
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

impl From<PlanValidationError> for PlanningError {
    fn from(e: PlanValidationError) -> Self {
        Self::InvalidPlanOutput(e.0)
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
    audit_log: Option<SafetyAuditLog>,
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
            audit_log: None,
        }
    }

    /// Attach an optional [`SafetyAuditLog`] for persistent logging of
    /// safety fence activations. When set, every `propose_plan` rejection
    /// is appended to the log file in addition to being printed to stderr.
    pub fn with_audit_log(mut self, log: SafetyAuditLog) -> Self {
        self.audit_log = Some(log);
        self
    }

    /// Construct a planner from a [`BrainConfig`].
    ///
    /// Uses Rig provider clients for all backends. Returns an error if the
    /// HTTP client cannot be initialised (rare; only fails if the TLS
    /// subsystem is unavailable).
    pub fn from_config(
        config: BrainConfig,
        state_client: Box<dyn StateClient>,
    ) -> Result<Self, String> {
        let provider: Box<dyn LlmProvider> = match config.provider {
            ProviderConfig::Anthropic {
                api_key,
                model,
                base_url,
            } => {
                let client = rig::providers::anthropic::Client::builder()
                    .api_key(api_key)
                    .base_url(base_url)
                    .build()
                    .map_err(|e| e.to_string())?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::Ollama { base_url, model } => {
                let client = rig::providers::ollama::Client::builder()
                    .api_key(rig::client::Nothing)
                    .base_url(base_url)
                    .build()
                    .map_err(|e| e.to_string())?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::OpenAI { api_key, model } => {
                let client = rig::providers::openai::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| e.to_string())?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::Gemini { api_key, model } => {
                let client = rig::providers::gemini::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| e.to_string())?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::Groq { api_key, model } => {
                let client = rig::providers::groq::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| e.to_string())?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::DeepSeek { api_key, model } => {
                let client = rig::providers::deepseek::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| e.to_string())?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::Mistral { api_key, model } => {
                let client = rig::providers::mistral::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| e.to_string())?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::XAI { api_key, model } => {
                let client = rig::providers::xai::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| e.to_string())?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
        };
        Ok(Self::new(provider, state_client, config.max_turns))
    }

    /// Expose the current system state from the underlying `StateClient`.
    ///
    /// Used by the Tauri commands layer to populate system-context fields in
    /// `PlanResponse` without requiring a second network call.
    pub fn curated_state(&self) -> Result<crate::state_client::CuratedState, PlanningError> {
        self.state_client.curated_state()
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

        for turn in 0..self.max_turns {
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
                                // Propagate serialisation errors: feeding `{}` to the LLM
                                // would cause it to plan against phantom data. In practice
                                // CuratedState is always serialisable (only String/Vec<String>
                                // fields), but this guards against future type changes.
                                let state_json = serde_json::to_string(&state).map_err(|e| {
                                    PlanningError::StateUnavailable(format!(
                                        "failed to serialize system state: {e}"
                                    ))
                                })?;
                                tool_results.push(ToolResultBlock {
                                    tool_use_id: id.clone(),
                                    content: state_json,
                                    is_error: false,
                                });
                            }
                            "propose_plan" => {
                                // Parse and validate before returning.
                                // If validation fails, log the rejection (safety fence
                                // activations are security-relevant events) and feed the
                                // error back as a tool result so the LLM can self-correct
                                // within the remaining turns. Symmetric with the
                                // unknown-tool retry path below.
                                match parse_proposed_plan(intent, input) {
                                    Ok(plan) => return Ok(plan),
                                    Err(e) => {
                                        let reason = e.to_string();
                                        let raw_plan = input.to_string();
                                        eprintln!(
                                            "[LACS SAFETY] propose_plan rejected \
                                             (turn {}/{max}): {reason}. Input: {raw_plan}",
                                            turn + 1,
                                            max = self.max_turns
                                        );
                                        if let Some(audit) = &self.audit_log {
                                            audit.log_rejection(intent, &reason, &raw_plan);
                                        }
                                        tool_results.push(ToolResultBlock {
                                            tool_use_id: id.clone(),
                                            content: format!(
                                                "Plan rejected: {reason}. \
                                                 Correct the plan and call propose_plan again."
                                            ),
                                            is_error: true,
                                        });
                                    }
                                }
                            }
                            unknown => {
                                // An unknown tool call is a protocol violation — log it
                                // as a safety event and feed the error back so the LLM
                                // has a chance to recover within the remaining turns.
                                eprintln!(
                                    "[LACS WARNING] LLM called unknown tool '{unknown}' \
                                     (turn {}/{max}); sending error feedback.",
                                    turn + 1,
                                    max = self.max_turns
                                );
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
