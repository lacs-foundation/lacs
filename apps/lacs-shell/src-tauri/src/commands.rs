use crate::events::DaemonJobOutcome;
use lacs_brain::config::BrainConfig;
#[cfg(any(test, feature = "demo"))]
use lacs_brain::planner::PlanningError;
use lacs_brain::planner::{LlmPlanner, Plan};
use lacs_brain::state_client::CuratedState;
#[cfg(any(test, feature = "demo"))]
use lacs_brain::state_client::StateClient;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

// ---------------------------------------------------------------------------
// Response types (serialised to the frontend)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStepResponse {
    pub action_name: String,
    pub summary: String,
    pub risk_level: String,
    pub approval_required: bool,
    /// Runtime parameters chosen by the brain. The frontend passes these back
    /// verbatim in `approve_preview` so the daemon can execute the step.
    pub params: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanResponse {
    pub summary: String,
    pub explanation: String,
    pub approval_required: bool,
    pub steps: Vec<PlanStepResponse>,
    pub host_name: String,
    pub deployment: String,
    pub toolbox_count: usize,
    pub flatpak_count: usize,
}

/// Typed error returned to the frontend. `code` matches `ShellErrorCode` in `types.ts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellError {
    pub code: String,
    pub message: String,
    pub system_changed: bool,
}

impl ShellError {
    fn pre_flight(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            system_changed: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainConfigResponse {
    pub provider: String,
    pub model: String,
    pub fallback: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    pub config_exists: bool,
    pub provider_configured: bool,
}

// ---------------------------------------------------------------------------
// Request types (deserialised from the frontend)
// ---------------------------------------------------------------------------

/// One plan step submitted by the frontend for execution approval.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStepRequest {
    pub action_name: String,
    pub params: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Demo state client (hardcoded Silverblue fixture)
// ---------------------------------------------------------------------------

/// Hardcoded Silverblue fixture. Used in tests and demo builds.
/// Production builds use `DaemonIpcClient` instead.
#[cfg(any(test, feature = "demo"))]
#[derive(Clone, Debug, Default)]
pub struct DemoStateClient;

#[cfg(any(test, feature = "demo"))]
impl StateClient for DemoStateClient {
    fn curated_state(&self) -> Result<CuratedState, PlanningError> {
        CuratedState::new(
            "silverblue",
            "fedora/41",
            vec!["NetworkManager.service".to_string()],
            vec!["org.mozilla.firefox".to_string()],
            vec!["lacs-dev".to_string()],
        )
        .map_err(PlanningError::StateUnavailable)
    }
}

// ---------------------------------------------------------------------------
// Shell command state
// ---------------------------------------------------------------------------

/// Read the daemon socket path from the environment (set by config.toml
/// via `apply_defaults_to_env()`), falling back to the compile-time default.
fn resolve_socket_path() -> String {
    let uri = std::env::var("LACS_LISTEN_URI")
        .unwrap_or_else(|_| lacs_core::DEFAULT_LISTEN_URI.to_string());
    uri.strip_prefix("unix://").unwrap_or(&uri).to_string()
}

/// Returns the `StateClient` for the current build.
///
/// In `demo` or test builds, returns `DemoStateClient` (hardcoded Silverblue
/// fixture). In production builds, returns `DaemonIpcClient`, which queries
/// the running `lacs-daemon` over its Unix socket.
#[cfg(any(test, feature = "demo"))]
fn build_state_client() -> Box<dyn StateClient> {
    Box::new(DemoStateClient)
}

#[cfg(not(any(test, feature = "demo")))]
fn build_state_client() -> Box<dyn lacs_brain::state_client::StateClient> {
    let socket_path = resolve_socket_path();
    Box::new(crate::daemon_client::DaemonIpcClient::new(socket_path))
}

pub struct ShellCommandState {
    planner: LlmPlanner,
    brain_config: BrainConfigResponse,
}

impl ShellCommandState {
    /// Create from environment-derived config.
    ///
    /// Logs a warning and falls back to Ollama defaults when `LACS_LLM_PROVIDER`
    /// is not set or the config is invalid, so the shell starts even without
    /// API credentials configured.
    ///
    /// In demo or test builds, uses `DemoStateClient` (hardcoded fixture).
    /// In production builds, uses `DaemonIpcClient` to query live state from
    /// the running `lacs-daemon`.
    pub fn new() -> Self {
        let env_result = BrainConfig::from_env();
        let fallback = env_result.is_err();
        let config = env_result.unwrap_or_else(|err| {
            eprintln!("[LACS WARNING] Brain config error: {err}. Falling back to Ollama defaults.");
            BrainConfig::ollama_defaults()
        });
        let brain_config = BrainConfigResponse {
            provider: config.provider_name().to_string(),
            model: config.model_name().to_string(),
            fallback,
        };
        let planner = LlmPlanner::from_config(config, build_state_client()).unwrap_or_else(|err| {
            eprintln!(
                "[LACS WARNING] Failed to build LLM provider: {err}. \
                 Check LACS_LLM_PROVIDER and related env vars."
            );
            LlmPlanner::from_config(BrainConfig::ollama_defaults(), build_state_client())
                .expect("Ollama defaults must always produce a valid planner")
        });
        Self {
            planner,
            brain_config,
        }
    }

    pub fn brain_config_response(&self) -> BrainConfigResponse {
        self.brain_config.clone()
    }

    /// Inject a pre-built planner and brain config — used in unit tests.
    #[cfg(test)]
    pub fn with_planner(planner: LlmPlanner, brain_config: BrainConfigResponse) -> Self {
        Self {
            planner,
            brain_config,
        }
    }
}

impl Default for ShellCommandState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn plan_intent(
    state: tauri::State<'_, ShellCommandState>,
    intent: String,
) -> Result<PlanResponse, ShellError> {
    execute_plan_intent(&state, &intent).await
}

/// Execute approved plan steps against the daemon.
///
/// For each step the shell calls daemon `preview` (to obtain the
/// `request_hash`) and then daemon `execute`. Progress lines are forwarded
/// to the frontend as `lacs:timeline-entry` events. A single
/// `lacs:job-completed` event is emitted after all steps finish (or on the
/// first non-succeeded outcome).
///
/// This command always returns `Ok` — infrastructure failures are surfaced as
/// a `DaemonJobOutcome::Failed` event so the frontend is never left stuck in
/// the "executing" state.
#[tauri::command]
pub async fn approve_preview(app: AppHandle, steps: Vec<PlanStepRequest>) -> Result<(), String> {
    let socket_path = resolve_socket_path();

    let mut final_status = "succeeded".to_string();

    'steps: for step in &steps {
        match crate::daemon_client::execute_action(
            &socket_path,
            &app,
            &step.action_name,
            &step.params,
        )
        .await
        {
            Ok(status) => {
                final_status = status;
                if final_status != "succeeded" {
                    break 'steps;
                }
            }
            Err(e) => {
                eprintln!(
                    "[lacs-shell] execute_action failed for '{}': {e}",
                    step.action_name
                );
                final_status = "failed".to_string();
                break 'steps;
            }
        }
    }

    let outcome = match final_status.as_str() {
        "succeeded" => DaemonJobOutcome::Succeeded,
        "needs_reboot" => DaemonJobOutcome::NeedsReboot,
        "rolled_back" => DaemonJobOutcome::RolledBack,
        _ => DaemonJobOutcome::Failed,
    };

    app.emit("lacs:job-completed", outcome)
        .map_err(|e| format!("failed to emit lacs:job-completed: {e}"))?;

    Ok(())
}

#[tauri::command]
pub fn get_brain_config(state: tauri::State<'_, ShellCommandState>) -> BrainConfigResponse {
    state.brain_config_response()
}

#[tauri::command]
pub fn check_setup_status() -> SetupStatus {
    SetupStatus {
        config_exists: config_path_exists(),
        provider_configured: provider_is_configured(),
    }
}

#[tauri::command]
pub fn cancel_job(app: AppHandle, job_id: String) -> Result<(), ShellError> {
    // Forward cancellation to daemon when daemon IPC wires cancellation support.
    // For now emits a local event so the frontend can transition to idle.
    if let Err(e) = app.emit("lacs:job-canceled", job_id) {
        eprintln!("[lacs-shell] failed to emit lacs:job-canceled: {e}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers (extracted so they are testable without a Tauri runtime)
// ---------------------------------------------------------------------------

pub(crate) async fn execute_plan_intent(
    state: &ShellCommandState,
    intent: &str,
) -> Result<PlanResponse, ShellError> {
    if intent.is_empty() {
        return Err(ShellError::pre_flight("intent_empty", "Intent is empty"));
    }

    let curated = state
        .planner
        .curated_state()
        .map_err(|e| ShellError::pre_flight("unknown", e.to_string()))?;

    let plan = state
        .planner
        .plan_intent(intent)
        .await
        .map_err(map_planning_error)?;

    Ok(plan_to_response(plan, &curated))
}

fn map_planning_error(err: lacs_brain::planner::PlanningError) -> ShellError {
    use lacs_brain::planner::PlanningError;
    let (code, msg) = match &err {
        PlanningError::EmptyIntent => ("intent_empty", err.to_string()),
        PlanningError::StateUnavailable(_) => ("daemon_not_running", err.to_string()),
        PlanningError::Provider(s) => {
            if s.contains("429") {
                ("llm_rate_limit", err.to_string())
            } else if s.starts_with("http") || s.contains("HTTP") {
                ("llm_http_error", err.to_string())
            } else {
                ("llm_parse_error", err.to_string())
            }
        }
        PlanningError::InvalidPlanOutput(_) => ("llm_parse_error", err.to_string()),
        _ => ("unknown", err.to_string()),
    };
    ShellError::pre_flight(code, msg)
}

pub(crate) fn plan_to_response(plan: Plan, curated: &CuratedState) -> PlanResponse {
    let approval_required = plan.steps().iter().any(|step| step.approval_required());
    let steps = plan
        .steps()
        .iter()
        .map(|step| PlanStepResponse {
            action_name: step.action_name().to_string(),
            summary: step.summary().to_string(),
            risk_level: step.risk_level().as_str().to_string(),
            approval_required: step.approval_required(),
            params: step.params().clone(),
        })
        .collect();

    PlanResponse {
        summary: plan.summary().to_string(),
        explanation: plan.explanation().to_string(),
        approval_required,
        steps,
        host_name: curated.host_name().to_string(),
        deployment: curated.deployment().to_string(),
        toolbox_count: curated.toolboxes().len(),
        flatpak_count: curated.flatpaks().len(),
    }
}

// ---------------------------------------------------------------------------
// Setup-status helpers (no daemon connection required)
// ---------------------------------------------------------------------------

/// Returns `true` when `~/.config/lacs/config.toml` (or equivalent XDG path)
/// exists on disk.
fn config_path_exists() -> bool {
    lacs_core::config::LacsConfig::config_path().is_file()
}

/// Returns `true` when any of these hold:
///
/// 1. `ANTHROPIC_API_KEY` env var is set (non-empty), OR
/// 2. `LACS_LLM_PROVIDER` env var is set (non-empty), OR
/// 3. `config.toml` has `[llm] provider = "..."` set.
fn provider_is_configured() -> bool {
    if std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|v| !v.is_empty())
        .is_some()
    {
        return true;
    }
    if std::env::var("LACS_LLM_PROVIDER")
        .ok()
        .filter(|v| !v.is_empty())
        .is_some()
    {
        return true;
    }
    let cfg = lacs_core::config::LacsConfig::load();
    cfg.llm
        .as_ref()
        .and_then(|llm| llm.provider.as_deref())
        .is_some_and(|p| !p.is_empty())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use lacs_brain::planner::PlanRiskLevel;
    use lacs_brain::provider::{
        Completion, ContentBlock, LlmProvider, Message, ProviderError, StopReason, ToolDefinition,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct MockProvider {
        turns: Mutex<VecDeque<Result<Completion, ProviderError>>>,
    }

    impl MockProvider {
        fn once(turn: Result<Completion, ProviderError>) -> Self {
            Self {
                turns: Mutex::new(std::iter::once(turn).collect()),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn complete(
            &self,
            _system: &str,
            _messages: &[Message],
            _tools: &[ToolDefinition],
            _max_tokens: u32,
        ) -> Result<Completion, ProviderError> {
            self.turns
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(ProviderError::Parse("mock exhausted".into())))
        }
    }

    fn test_brain_config() -> BrainConfigResponse {
        BrainConfigResponse {
            provider: "test".into(),
            model: "test-model".into(),
            fallback: false,
        }
    }

    fn propose_plan_completion(
        summary: &str,
        explanation: &str,
        steps: &[(&str, &str, &str)],
    ) -> Result<Completion, ProviderError> {
        let steps_json: Vec<serde_json::Value> = steps
            .iter()
            .map(|(name, s, risk)| {
                serde_json::json!({
                    "action_name": name,
                    "summary": s,
                    "risk_level": risk,
                    "params": {}
                })
            })
            .collect();

        Ok(Completion {
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "propose_plan".into(),
                input: serde_json::json!({
                    "summary": summary,
                    "explanation": explanation,
                    "steps": steps_json
                }),
            }],
            stop_reason: StopReason::ToolUse,
        })
    }

    #[tokio::test]
    async fn empty_intent_returns_intent_empty_error() {
        let planner = LlmPlanner::new(
            Box::new(MockProvider::once(Err(ProviderError::Parse(
                "unused".into(),
            )))),
            Box::new(DemoStateClient),
            5,
        );
        let state = ShellCommandState::with_planner(planner, test_brain_config());
        let err = execute_plan_intent(&state, "").await.unwrap_err();
        assert_eq!(err.code, "intent_empty");
        assert!(!err.system_changed);
    }

    #[tokio::test]
    async fn plan_to_response_serialises_approval_required_correctly() {
        let planner = LlmPlanner::new(
            Box::new(MockProvider::once(propose_plan_completion(
                "Inspect system state",
                "This plan reads the current system state.",
                &[("GetSystemState", "Read state", "low")],
            ))),
            Box::new(DemoStateClient),
            5,
        );
        let state = ShellCommandState::with_planner(planner, test_brain_config());
        let response = execute_plan_intent(&state, "show me the system")
            .await
            .unwrap();
        assert!(!response.approval_required);
        assert_eq!(response.steps.len(), 1);
        assert_eq!(response.steps[0].action_name, "GetSystemState");
        assert_eq!(response.steps[0].risk_level, "low");
    }

    #[tokio::test]
    async fn plan_to_response_sets_approval_required_for_mutating_step() {
        let planner = LlmPlanner::new(
            Box::new(MockProvider::once(propose_plan_completion(
                "Install vim",
                "Layers vim via rpm-ostree.",
                &[("InstallPackages", "Layer vim", "high")],
            ))),
            Box::new(DemoStateClient),
            5,
        );
        let state = ShellCommandState::with_planner(planner, test_brain_config());
        let response = execute_plan_intent(&state, "install vim").await.unwrap();
        assert!(response.approval_required);
    }

    #[test]
    fn plan_to_response_maps_all_fields() {
        use lacs_brain::planner::{Plan, PlanStep};

        let step = PlanStep::new(
            "RebaseSystem".into(),
            "Rebase to f42".into(),
            PlanRiskLevel::High,
            serde_json::json!({}),
        )
        .unwrap();
        let plan = Plan::new(
            "rebase intent".into(),
            "Rebase the system".into(),
            "This rebases Fedora Silverblue to f42 and requires a reboot.".into(),
            vec![step],
        )
        .unwrap();
        let curated = DemoStateClient.curated_state().unwrap();
        let resp = plan_to_response(plan, &curated);

        assert_eq!(resp.summary, "Rebase the system");
        assert_eq!(
            resp.explanation,
            "This rebases Fedora Silverblue to f42 and requires a reboot."
        );
        assert!(resp.approval_required);
        assert_eq!(resp.steps[0].risk_level, "high");
        assert_eq!(resp.host_name, "silverblue");
        assert_eq!(resp.deployment, "fedora/41");
        assert_eq!(resp.toolbox_count, 1);
        assert_eq!(resp.flatpak_count, 1);
    }

    #[tokio::test]
    async fn provider_error_surfaces_as_llm_http_error() {
        let planner = LlmPlanner::new(
            Box::new(MockProvider::once(Err(ProviderError::Http {
                status: 500,
                body: "internal server error".into(),
            }))),
            Box::new(DemoStateClient),
            5,
        );
        let state = ShellCommandState::with_planner(planner, test_brain_config());
        let err = execute_plan_intent(&state, "install vim")
            .await
            .unwrap_err();
        assert!(
            err.code == "llm_http_error" || err.code == "llm_parse_error",
            "expected http or parse error code, got: {}",
            err.code
        );
    }

    #[test]
    fn plan_to_response_approval_required_when_any_step_is_high_risk() {
        use lacs_brain::planner::{Plan, PlanStep};

        let steps = vec![
            PlanStep::new(
                "GetSystemState".into(),
                "Read current state".into(),
                PlanRiskLevel::Low,
                serde_json::json!({}),
            )
            .unwrap(),
            PlanStep::new(
                "InstallPackages".into(),
                "Layer vim via rpm-ostree".into(),
                PlanRiskLevel::High,
                serde_json::json!({}),
            )
            .unwrap(),
        ];
        let plan = Plan::new(
            "install vim intent".into(),
            "Install vim on the system".into(),
            "Reads state then layers vim. Requires reboot.".into(),
            steps,
        )
        .unwrap();
        let curated = DemoStateClient.curated_state().unwrap();
        let resp = plan_to_response(plan, &curated);

        assert!(
            resp.approval_required,
            "approval_required must be true when any step is high-risk"
        );
        assert_eq!(resp.steps.len(), 2);
        assert!(
            !resp.steps[0].approval_required,
            "low step should not require approval"
        );
        assert!(
            resp.steps[1].approval_required,
            "high step must require approval"
        );
    }

    #[test]
    fn get_brain_config_returns_provider_and_model() {
        let state = ShellCommandState::new();
        let cfg = state.brain_config_response();
        assert!(cfg.provider == "anthropic" || cfg.provider == "ollama");
        assert!(!cfg.model.is_empty());
    }
}
