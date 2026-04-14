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
use crate::planning_tools::query_tools::query_tools;
use crate::prompt::build_system_prompt;
use crate::provider::{
    ContentBlock, LlmProvider, Message, ProviderError, Role, StopReason, ToolDefinition,
    ToolResultBlock,
};
use crate::providers::openai_adapter::AsyncOpenAiAdapter;
use crate::providers::rig_adapter::RigCompletionAdapter;
use crate::state_client::StateClient;
use rig::client::CompletionClient;
use serde::Serialize;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Ollama-provider tuning constants
// ---------------------------------------------------------------------------

/// Output token budget passed to Ollama as `options.num_predict`.
///
/// Why this is needed at all: Rig's `OllamaCompletionRequest` sends
/// `max_tokens` at the top level of the JSON body, which Ollama's
/// `/api/chat` endpoint **ignores**. Ollama reads `options.num_predict`
/// for the generation limit. The `RigCompletionAdapter::with_additional_params`
/// keys (other than `think`/`keep_alive`, which the Ollama provider
/// consumes as top-level fields) flow into `options`, so writing
/// `num_predict` there lands it in the right place.
///
/// Why this specific value: we need enough headroom for:
///   - a thinking trace (qwen3 typically emits 100–400 tokens),
///   - a complete `propose_plan` tool-call JSON (150–300 tokens),
///   - a small buffer for retries and fallbacks.
///
/// 4096 covers the worst case comfortably while staying below values
/// that would let the model wander for minutes of thinking on CPU.
/// Empirically, well-behaved LACS runs never approach this limit;
/// untuned models that *do* approach it are the ones we cannot use
/// anyway (CPU inference hits Ollama's internal request timeout first).
pub const OLLAMA_NUM_PREDICT: u32 = 4096;

/// Maximum output tokens for the planning loop.
///
/// Must be large enough for: a thinking trace (100–400 tokens),
/// a `propose_plan` tool-call JSON (150–300 tokens), and a
/// buffer for multi-turn retries. 4096 is generous for all
/// providers — well-behaved runs rarely exceed 1000.
pub const PLANNING_MAX_TOKENS: u32 = 4096;

/// Maximum output tokens for the summarization endpoint.
///
/// Summarization produces short plain-language text (no tools,
/// no structured output). 512 tokens is ample for a one-paragraph
/// summary of daemon execution output.
pub const SUMMARIZATION_MAX_TOKENS: u32 = 512;

/// Model-name prefixes that signal thinking-mode capability in Ollama.
///
/// Source of truth: Ollama documents which models accept the `think`
/// field on `/api/chat`. Sending `think: true` to a non-thinking model
/// returns HTTP 400 with `"does not support thinking"`. This list
/// therefore must be kept conservative — add a prefix only after
/// verifying the model's tag + Ollama version combination accepts it.
///
/// Current entries, verified live:
///   - `qwen3`    — all Qwen3 variants (0.6b … 30b-a3b)
///   - `qwq`      — Qwen reasoning-focused variant (qwq:32b)
///   - `deepseek-r` — DeepSeek-R1 family
///
/// NOT listed (do not support thinking): `llama3.2`, `gemma3`,
/// `qwen2.5`, `mistral`, `gemma2`.
///
/// An out-of-process override lives in `LACS_OLLAMA_THINK`; this
/// auto-detection is only the default.
pub const THINKING_MODEL_PREFIXES: &[&str] = &["qwen3", "qwq", "deepseek-r"];

/// Environment variable that overrides the auto-detected thinking mode.
///
/// Set to `"true"` or `"false"` (case-insensitive). Any other value
/// falls back to auto-detection. Populated by `LacsConfig` from
/// `config.toml`'s `[llm] ollama_think` field.
pub const LACS_OLLAMA_THINK_ENV: &str = "LACS_OLLAMA_THINK";

/// Decide whether to send `think: true` for a given Ollama model.
///
/// Resolution order (highest priority wins):
///   1. `LACS_OLLAMA_THINK` env var, if set to a parseable `true`/`false`.
///   2. Auto-detection against [`THINKING_MODEL_PREFIXES`].
///
/// An unparseable env-var value (neither `"true"` nor `"false"` after
/// trimming and lowercasing) is ignored — we fall back to auto-detection
/// so a typo does not silently break tool use.
///
/// The distinction matters on CPU-only hosts: thinking models on 4 vCPUs
/// emit long reasoning traces that exceed Ollama's request timeout before
/// any tool call lands. Users on CPU should set `ollama_think = false`
/// in `config.toml` for qwen3-class models; this helper respects that.
pub fn resolve_ollama_think(model: &str) -> bool {
    if let Ok(raw) = std::env::var(LACS_OLLAMA_THINK_ENV) {
        match raw.trim().to_lowercase().as_str() {
            "true" => return true,
            "false" => return false,
            _ => {
                // Unparseable override — fall through to auto-detection.
                // We intentionally do not log this; startup noise is not
                // worth it and the auto-detected behaviour is safe.
            }
        }
    }
    let model_lower = model.to_lowercase();
    THINKING_MODEL_PREFIXES
        .iter()
        .any(|prefix| model_lower.starts_with(prefix))
}

// ---------------------------------------------------------------------------
// PlanEvent
// ---------------------------------------------------------------------------

/// Progress events emitted by the LLM planning loop.
///
/// Consumers (e.g. the `lacs` CLI) subscribe via an
/// `tokio::sync::mpsc::UnboundedSender<PlanEvent>` and update a spinner
/// message in real time.  Events are fire-and-forget; a closed channel is
/// silently ignored.
#[derive(Debug, Clone)]
pub enum PlanEvent {
    /// The planner sent the first prompt to the LLM.
    Thinking,
    /// The LLM called a query or state tool by the given name.
    QueryingTool(String),
    /// The LLM called `propose_plan` with a valid proposal.
    ProposingPlan,
}

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
/// Tool definitions are built once at construction time and reused across all
/// planning calls. The system prompt is rebuilt per `plan_intent()` call to
/// include current user preferences.
pub struct LlmPlanner {
    provider: Box<dyn LlmProvider>,
    state_client: Box<dyn StateClient>,
    max_turns: usize,
    tools: Vec<ToolDefinition>,
    audit_log: Option<SafetyAuditLog>,
    prefs_path: Option<std::path::PathBuf>,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<PlanEvent>>,
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
            tools: {
                let mut t = vec![get_state_tool_def()];
                t.extend(query_tools());
                t.push(crate::planning_tools::preferences::remember_tool_def());
                t.push(crate::planning_tools::preferences::forget_tool_def());
                t.push(propose_plan_tool_def());
                t
            },
            audit_log: None,
            prefs_path: None,
            progress_tx: None,
        }
    }

    /// Attach an optional [`SafetyAuditLog`] for persistent logging of
    /// safety fence activations. When set, every `propose_plan` rejection
    /// is appended to the log file in addition to being printed to stderr.
    pub fn with_audit_log(mut self, log: SafetyAuditLog) -> Self {
        self.audit_log = Some(log);
        self
    }

    /// Set the path to the user preferences file.
    ///
    /// When set, preferences are read at the start of each `plan_intent()`
    /// call and injected into the system prompt. The `remember` and `forget`
    /// tools write to this file.
    pub fn with_prefs_path(mut self, path: std::path::PathBuf) -> Self {
        self.prefs_path = Some(path);
        self
    }

    /// Attach a progress channel for real-time planning feedback.
    ///
    /// The planner emits [`PlanEvent`]s on `tx` as it progresses through the
    /// tool-use loop. When `plan_intent` returns, the sender is dropped,
    /// closing the channel so consumers can detect completion naturally.
    pub fn with_progress(mut self, tx: tokio::sync::mpsc::UnboundedSender<PlanEvent>) -> Self {
        self.progress_tx = Some(tx);
        self
    }

    /// Send a [`PlanEvent`] to the progress channel, if one is attached.
    ///
    /// A closed or absent channel is silently ignored — progress events are
    /// advisory and must never affect planning behaviour.
    fn emit(&self, event: PlanEvent) {
        if let Some(ref tx) = self.progress_tx {
            let _ = tx.send(event);
        }
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
                    .map_err(|e| format!("failed to initialize anthropic provider: {e}"))?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::Ollama { base_url, model } => {
                let client = rig::providers::ollama::Client::builder()
                    .api_key(rig::client::Nothing)
                    .base_url(base_url)
                    .build()
                    .map_err(|e| format!("failed to initialize ollama provider: {e}"))?;
                let completion_model = client.completion_model(&model);
                // See `OLLAMA_NUM_PREDICT` and `THINKING_MODEL_PREFIXES`
                // at the top of this module for the rationale behind each
                // key sent through `additional_params`.
                let think = resolve_ollama_think(&model);
                let mut params = serde_json::json!({ "num_predict": OLLAMA_NUM_PREDICT });
                if think {
                    params["think"] = serde_json::Value::Bool(true);
                }
                Box::new(RigCompletionAdapter::new(completion_model).with_additional_params(params))
            }
            ProviderConfig::OpenAI { api_key, model } => {
                // Use async-openai directly with the Chat Completions API.
                // rig's OpenAI backend defaults to the Responses API, which:
                //   - emits reasoning-only items on some gpt-4o variants → parse errors
                //   - places the system prompt in a user message (rig issue #1599)
                // async-openai targets /v1/chat/completions, has none of these issues.
                Box::new(AsyncOpenAiAdapter::new(api_key, model))
            }
            ProviderConfig::Gemini { api_key, model } => {
                let client = rig::providers::gemini::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| format!("failed to initialize gemini provider: {e}"))?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::Groq { api_key, model } => {
                let client = rig::providers::groq::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| format!("failed to initialize groq provider: {e}"))?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::DeepSeek { api_key, model } => {
                let client = rig::providers::deepseek::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| format!("failed to initialize deepseek provider: {e}"))?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::Mistral { api_key, model } => {
                let client = rig::providers::mistral::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| format!("failed to initialize mistral provider: {e}"))?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
            ProviderConfig::XAI { api_key, model } => {
                let client = rig::providers::xai::Client::builder()
                    .api_key(api_key)
                    .build()
                    .map_err(|e| format!("failed to initialize xai provider: {e}"))?;
                let completion_model = client.completion_model(&model);
                Box::new(RigCompletionAdapter::new(completion_model))
            }
        };
        let mut planner = Self::new(provider, state_client, config.max_turns);
        planner.prefs_path = Some(lacs_core::config::prefs_path());
        Ok(planner)
    }

    /// Expose the current system state from the underlying `StateClient`.
    ///
    /// Used by the Tauri commands layer to populate system-context fields in
    /// `PlanResponse` without requiring a second network call.
    pub fn curated_state(&self) -> Result<crate::state_client::CuratedState, PlanningError> {
        self.state_client.curated_state()
    }

    /// Generate a plain-language summary of a short prompt, bypassing the
    /// tool-use loop. Used for post-execution review.
    ///
    /// Returns the raw text content from the LLM. No tools are provided, so
    /// the LLM is constrained to text-only output.
    pub async fn summarize(&self, prompt: &str) -> Result<String, PlanningError> {
        let messages = vec![Message::user_text(prompt)];
        let completion = self
            .provider
            .complete(
                "You are a concise technical writer. Respond with a short plain-language summary. Do not use markdown formatting.",
                &messages,
                &[], // no tools
                SUMMARIZATION_MAX_TOKENS,
            )
            .await
            .map_err(PlanningError::from)?;

        // Extract text from the completion
        let text = completion
            .content
            .iter()
            .filter_map(|b| {
                if let ContentBlock::Text { text } = b {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        if text.is_empty() {
            Err(PlanningError::NoPlanProposed)
        } else {
            Ok(text)
        }
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

        // Rebuild the system prompt with current preferences on each call
        // so that preferences saved during a prior `plan_intent` are visible.
        let effective_prompt = {
            let prefs_content = match self.prefs_path.as_ref() {
                Some(p) => match crate::prefs::read_prefs(p) {
                    Ok(content) => content,
                    Err(e) => {
                        eprintln!(
                            "[lacs-brain] failed to read preferences from {}: {e}",
                            p.display()
                        );
                        None
                    }
                },
                None => None,
            };
            build_system_prompt(prefs_content.as_deref())
        };

        for turn in 0..self.max_turns {
            self.emit(PlanEvent::Thinking);
            let completion = self
                .provider
                .complete(&effective_prompt, &messages, &self.tools, PLANNING_MAX_TOKENS)
                .await
                .map_err(PlanningError::from)?;

            messages.push(Message {
                role: Role::Assistant,
                content: completion.content.clone(),
            });

            match completion.stop_reason {
                StopReason::MaxTokens => {
                    return Err(PlanningError::NoPlanProposed);
                }
                StopReason::EndTurn => {
                    // Some providers (e.g. Gemini via rig) may output the plan
                    // as a plain-text JSON block instead of calling propose_plan.
                    // Inject a correction and let the model retry — but only if
                    // we have turns remaining.
                    let has_text = completion
                        .content
                        .iter()
                        .any(|b| matches!(b, ContentBlock::Text { .. }));
                    if has_text && turn + 1 < self.max_turns {
                        messages.push(Message::user_text(
                            "You must call the `propose_plan` tool. \
                             Do not output JSON or text directly — \
                             your response must be a tool call to `propose_plan`.",
                        ));
                        continue;
                    }
                    if has_text {
                        eprintln!(
                            "[lacs-brain] LLM returned text instead of propose_plan on \
                             the final turn (turn {}/{max}); discarding output.",
                            turn + 1,
                            max = self.max_turns
                        );
                    }
                    return Err(PlanningError::NoPlanProposed);
                }
                StopReason::ToolUse => {
                    let tool_calls: Vec<_> = completion
                        .content
                        .iter()
                        .filter_map(|b| {
                            if let ContentBlock::ToolUse {
                                id,
                                call_id,
                                name,
                                input,
                            } = b
                            {
                                Some((id.clone(), call_id.clone(), name.clone(), input.clone()))
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

                    for (id, call_id, name, input) in &tool_calls {
                        // Emit a progress event before dispatching each tool.
                        self.emit(match name.as_str() {
                            "propose_plan"     => PlanEvent::ProposingPlan,
                            "get_system_state" => PlanEvent::QueryingTool("system state".into()),
                            other              => PlanEvent::QueryingTool(other.replace('_', " ")),
                        });

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
                                    call_id: call_id.clone(),
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
                                            call_id: call_id.clone(),
                                            content: format!(
                                                "Plan rejected: {reason}. \
                                                 Correct the plan and call propose_plan again."
                                            ),
                                            is_error: true,
                                        });
                                    }
                                }
                            }
                            "remember" => {
                                let fact = input.get("fact").and_then(|v| v.as_str()).unwrap_or("");
                                let (result_text, err) = if fact.is_empty() {
                                    (
                                        "Error: 'fact' parameter must not be empty.".to_string(),
                                        true,
                                    )
                                } else if crate::prefs::contains_sensitive(fact) {
                                    (
                                        "Error: preference rejected — it appears to contain \
                                         sensitive data (passwords, tokens, keys). Preferences \
                                         must not store secrets."
                                            .to_string(),
                                        true,
                                    )
                                } else if let Some(ref prefs_path) = self.prefs_path {
                                    match crate::prefs::append_pref(prefs_path, fact) {
                                        Ok(()) => (format!("Preference saved: {fact}"), false),
                                        Err(e) => {
                                            eprintln!(
                                                "[lacs-brain] failed to save preference to {}: {e}",
                                                prefs_path.display()
                                            );
                                            (format!("Error saving preference: {e}"), true)
                                        }
                                    }
                                } else {
                                    (
                                        "Error: preference storage is not configured.".to_string(),
                                        true,
                                    )
                                };
                                tool_results.push(ToolResultBlock {
                                    tool_use_id: id.clone(),
                                    call_id: call_id.clone(),
                                    content: result_text,
                                    is_error: err,
                                });
                            }
                            "forget" => {
                                let fact = input.get("fact").and_then(|v| v.as_str()).unwrap_or("");
                                let (result_text, err) = if fact.is_empty() {
                                    (
                                        "Error: 'fact' parameter must not be empty.".to_string(),
                                        true,
                                    )
                                } else if let Some(ref prefs_path) = self.prefs_path {
                                    match crate::prefs::remove_pref(prefs_path, fact) {
                                        Ok(true) => (format!("Preference removed: {fact}"), false),
                                        Ok(false) => {
                                            (format!("Preference not found: {fact}"), false)
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[lacs-brain] failed to remove preference from {}: {e}",
                                                prefs_path.display()
                                            );
                                            (format!("Error removing preference: {e}"), true)
                                        }
                                    }
                                } else {
                                    (
                                        "Error: preference storage is not configured.".to_string(),
                                        true,
                                    )
                                };
                                tool_results.push(ToolResultBlock {
                                    tool_use_id: id.clone(),
                                    call_id: call_id.clone(),
                                    content: result_text,
                                    is_error: err,
                                });
                            }
                            other_name => {
                                match crate::planning_tools::query_tools::query_tool_to_action(
                                    other_name, input,
                                ) {
                                    Ok(Some((action_name, params))) => {
                                        match self.state_client.query_action(action_name, &params) {
                                            Ok(output) => {
                                                tool_results.push(ToolResultBlock {
                                                    tool_use_id: id.clone(),
                                                    call_id: call_id.clone(),
                                                    content: output,
                                                    is_error: false,
                                                });
                                            }
                                            Err(e) => {
                                                tool_results.push(ToolResultBlock {
                                                    tool_use_id: id.clone(),
                                                    call_id: call_id.clone(),
                                                    content: format!("Query failed: {e}"),
                                                    is_error: true,
                                                });
                                            }
                                        }
                                    }
                                    Err(msg) => {
                                        // Missing required param — give the LLM a clear,
                                        // actionable message so it can retry correctly.
                                        tool_results.push(ToolResultBlock {
                                            tool_use_id: id.clone(),
                                            call_id: call_id.clone(),
                                            content: msg,
                                            is_error: true,
                                        });
                                    }
                                    Ok(None) => {
                                        // An unknown tool call is a protocol violation — log
                                        // it as a safety event and feed the error back so the
                                        // LLM has a chance to recover within the remaining
                                        // turns.
                                        eprintln!(
                                            "[LACS WARNING] LLM called unknown tool \
                                             '{other_name}' (turn {}/{max}); sending error \
                                             feedback.",
                                            turn + 1,
                                            max = self.max_turns
                                        );
                                        tool_results.push(ToolResultBlock {
                                            tool_use_id: id.clone(),
                                            call_id: call_id.clone(),
                                            content: format!("unknown tool: {other_name}"),
                                            is_error: true,
                                        });
                                    }
                                }
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

// ---------------------------------------------------------------------------
// Unit tests (module-local helpers only — integration tests live in
// crates/lacs-brain/tests/planner.rs).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env-var mutation is process-global; tests that touch it must be
    // serialised to avoid cross-test interference on a multi-threaded
    // test runner.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_think_auto_detects_qwen3() {
        let _g = ENV_LOCK.lock().unwrap();
        // SAFETY: single-threaded within this test under ENV_LOCK.
        unsafe { std::env::remove_var(LACS_OLLAMA_THINK_ENV) };
        assert!(resolve_ollama_think("qwen3:8b"));
        assert!(resolve_ollama_think("Qwen3:30b-a3b"));
        assert!(resolve_ollama_think("qwq:32b"));
        assert!(resolve_ollama_think("deepseek-r1:7b"));
    }

    #[test]
    fn resolve_think_auto_detects_non_thinking_models() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var(LACS_OLLAMA_THINK_ENV) };
        assert!(!resolve_ollama_think("llama3.2:3b"));
        assert!(!resolve_ollama_think("gemma3:1b"));
        assert!(!resolve_ollama_think("qwen2.5:3b"));
        assert!(!resolve_ollama_think("mistral-small3.2:24b"));
    }

    #[test]
    fn resolve_think_env_override_true_wins_over_non_thinking_model() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var(LACS_OLLAMA_THINK_ENV, "true") };
        let got = resolve_ollama_think("llama3.2:3b");
        unsafe { std::env::remove_var(LACS_OLLAMA_THINK_ENV) };
        assert!(got, "env override should force think=true");
    }

    #[test]
    fn resolve_think_env_override_false_wins_over_thinking_model() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var(LACS_OLLAMA_THINK_ENV, "false") };
        let got = resolve_ollama_think("qwen3:8b");
        unsafe { std::env::remove_var(LACS_OLLAMA_THINK_ENV) };
        assert!(!got, "env override should force think=false");
    }

    #[test]
    fn resolve_think_env_override_case_insensitive() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var(LACS_OLLAMA_THINK_ENV, "  TRUE  ") };
        let got = resolve_ollama_think("llama3.2:3b");
        unsafe { std::env::remove_var(LACS_OLLAMA_THINK_ENV) };
        assert!(got);
    }

    #[test]
    fn resolve_think_unparseable_env_falls_back_to_auto() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var(LACS_OLLAMA_THINK_ENV, "yes") };
        let qwen = resolve_ollama_think("qwen3:8b");
        let llama = resolve_ollama_think("llama3.2:3b");
        unsafe { std::env::remove_var(LACS_OLLAMA_THINK_ENV) };
        assert!(qwen, "unparseable value should NOT disable auto-detection");
        assert!(!llama, "unparseable value should NOT force think on");
    }
}
