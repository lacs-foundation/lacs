//! Integration tests for `LlmPlanner`.
//!
//! All tests use `MockProvider` and `MockStateClient` — no network calls.
//! The `MockProvider` returns a pre-configured sequence of `Completion` values.
//! Async tests use `#[tokio::test]`; synchronous error-message stability tests
//! do not require a runtime.

use async_trait::async_trait;
use lacs_brain::planner::{LlmPlanner, PlanningError};
use lacs_brain::provider::{
    Completion, ContentBlock, LlmProvider, Message, ProviderError, StopReason, ToolDefinition,
};
use lacs_brain::state_client::{CuratedState, StateClient};
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

struct MockProvider {
    turns: Mutex<VecDeque<Result<Completion, ProviderError>>>,
}

impl MockProvider {
    fn new(turns: impl IntoIterator<Item = Result<Completion, ProviderError>>) -> Self {
        Self {
            turns: Mutex::new(turns.into_iter().collect()),
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
            .unwrap_or_else(|| Err(ProviderError::Parse("mock provider exhausted".into())))
    }
}

#[derive(Default, Clone)]
struct MockStateClient {
    call_count: Arc<AtomicUsize>,
}

impl StateClient for MockStateClient {
    fn curated_state(&self) -> Result<CuratedState, PlanningError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(CuratedState::new(
            "silverblue",
            "fedora/41",
            vec!["NetworkManager.service".into()],
            vec!["org.mozilla.firefox".into()],
            vec!["lacs-dev".into()],
        ))
    }
}

struct FailingStateClient {
    reason: String,
}

impl StateClient for FailingStateClient {
    fn curated_state(&self) -> Result<CuratedState, PlanningError> {
        Err(PlanningError::StateUnavailable(self.reason.clone()))
    }
}

// ---------------------------------------------------------------------------
// Completion builders
// ---------------------------------------------------------------------------

fn propose_plan(
    summary: &str,
    steps: &[(&str, &str, &str)],
) -> Result<Completion, ProviderError> {
    let steps_json: Vec<serde_json::Value> = steps
        .iter()
        .map(|(name, step_summary, risk)| {
            serde_json::json!({
                "action_name": name,
                "summary": step_summary,
                "risk_level": risk,
                "params": {}
            })
        })
        .collect();

    Ok(Completion {
        content: vec![ContentBlock::ToolUse {
            id: "tu_001".into(),
            name: "propose_plan".into(),
            input: serde_json::json!({
                "summary": summary,
                "explanation": "Test plan explanation.",
                "steps": steps_json
            }),
        }],
        stop_reason: StopReason::ToolUse,
    })
}

fn get_system_state_call() -> Result<Completion, ProviderError> {
    Ok(Completion {
        content: vec![ContentBlock::ToolUse {
            id: "tu_state".into(),
            name: "get_system_state".into(),
            input: serde_json::json!({}),
        }],
        stop_reason: StopReason::ToolUse,
    })
}

fn end_turn_text(text: &str) -> Result<Completion, ProviderError> {
    Ok(Completion {
        content: vec![ContentBlock::Text { text: text.into() }],
        stop_reason: StopReason::EndTurn,
    })
}

fn make_planner(provider: MockProvider) -> LlmPlanner {
    LlmPlanner::new(
        Box::new(provider),
        Box::new(MockStateClient::default()),
        5,
    )
}

fn make_planner_with_state<S: StateClient + 'static>(
    provider: MockProvider,
    state: S,
) -> LlmPlanner {
    LlmPlanner::new(Box::new(provider), Box::new(state), 5)
}

// ---------------------------------------------------------------------------
// Empty / whitespace intent — guarded before any provider call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_intent_returns_error_without_calling_provider() {
    let planner = make_planner(MockProvider::new([]));
    assert_eq!(
        planner.plan_intent("").await.unwrap_err(),
        PlanningError::EmptyIntent
    );
}

#[tokio::test]
async fn whitespace_only_intent_returns_empty_intent_error() {
    let planner = make_planner(MockProvider::new([]));
    assert_eq!(
        planner.plan_intent("   \t\n  ").await.unwrap_err(),
        PlanningError::EmptyIntent
    );
}

// ---------------------------------------------------------------------------
// Single-turn: propose_plan returned immediately
// ---------------------------------------------------------------------------

#[tokio::test]
async fn single_turn_propose_plan_returns_plan() {
    let planner = make_planner(MockProvider::new([propose_plan(
        "Inspect system state",
        &[("GetSystemState", "Read current deployment info", "low")],
    )]));

    let plan = planner.plan_intent("show me the system").await.unwrap();

    assert_eq!(plan.intent(), "show me the system");
    assert_eq!(plan.steps().len(), 1);
    assert_eq!(plan.steps()[0].action_name(), "GetSystemState");
    assert!(!plan.steps()[0].approval_required());
    assert_eq!(plan.steps()[0].risk_level().as_str(), "low");
}

#[tokio::test]
async fn plan_carries_summary_and_explanation() {
    let planner = make_planner(MockProvider::new([propose_plan(
        "Read-only inspection",
        &[("GetSystemState", "Read state", "low")],
    )]));

    let plan = planner.plan_intent("inspect").await.unwrap();

    assert_eq!(plan.summary(), "Read-only inspection");
    assert_eq!(plan.explanation(), "Test plan explanation.");
}

// ---------------------------------------------------------------------------
// Two-turn: get_system_state first, then propose_plan
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_turn_state_then_plan_works() {
    let client = MockStateClient::default();
    let call_count = client.call_count.clone();

    let planner = make_planner_with_state(
        MockProvider::new([
            get_system_state_call(),
            propose_plan(
                "Install Firefox",
                &[("InstallFlatpak", "Install Firefox from Flathub", "medium")],
            ),
        ]),
        client,
    );

    let plan = planner.plan_intent("install firefox").await.unwrap();

    assert_eq!(plan.steps()[0].action_name(), "InstallFlatpak");
    assert!(plan.steps()[0].approval_required());
    assert_eq!(call_count.load(Ordering::Relaxed), 1);
}

// ---------------------------------------------------------------------------
// Risk level → approval_required derivation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn low_risk_step_has_no_approval_required() {
    let planner = make_planner(MockProvider::new([propose_plan(
        "Read only",
        &[("ListServices", "List all services", "low")],
    )]));
    let plan = planner.plan_intent("list services").await.unwrap();
    assert!(!plan.steps()[0].approval_required());
}

#[tokio::test]
async fn medium_risk_step_requires_approval() {
    let planner = make_planner(MockProvider::new([propose_plan(
        "Configure wifi",
        &[("ConfigureWifi", "Connect to home wifi", "medium")],
    )]));
    let plan = planner.plan_intent("connect to wifi").await.unwrap();
    assert!(plan.steps()[0].approval_required());
}

#[tokio::test]
async fn high_risk_step_requires_approval() {
    let planner = make_planner(MockProvider::new([propose_plan(
        "Rebase system",
        &[("RebaseSystem", "Rebase to Fedora 42", "high")],
    )]));
    let plan = planner.plan_intent("rebase to fedora 42").await.unwrap();
    assert!(plan.steps()[0].approval_required());
}

// ---------------------------------------------------------------------------
// Multi-step plan
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multi_step_plan_preserves_order_and_approval_flags() {
    let planner = make_planner(MockProvider::new([propose_plan(
        "Layer vim and reboot",
        &[
            ("GetSystemState", "Check current state", "low"),
            ("InstallPackages", "Layer vim package", "high"),
            ("RebootSystem", "Reboot into new deployment", "high"),
        ],
    )]));

    let plan = planner
        .plan_intent("layer vim and reboot")
        .await
        .unwrap();

    assert_eq!(plan.steps().len(), 3);
    assert_eq!(plan.steps()[0].action_name(), "GetSystemState");
    assert!(!plan.steps()[0].approval_required());
    assert_eq!(plan.steps()[1].action_name(), "InstallPackages");
    assert!(plan.steps()[1].approval_required());
    assert_eq!(plan.steps()[2].action_name(), "RebootSystem");
    assert!(plan.steps()[2].approval_required());
}

// ---------------------------------------------------------------------------
// params passthrough
// ---------------------------------------------------------------------------

#[tokio::test]
async fn plan_step_carries_params() {
    let planner = make_planner(MockProvider::new([Ok(Completion {
        content: vec![ContentBlock::ToolUse {
            id: "tu_p".into(),
            name: "propose_plan".into(),
            input: serde_json::json!({
                "summary": "Install vim",
                "explanation": "Layers vim.",
                "steps": [{
                    "action_name": "InstallPackages",
                    "summary": "Layer vim",
                    "risk_level": "high",
                    "params": { "packages": ["vim"] }
                }]
            }),
        }],
        stop_reason: StopReason::ToolUse,
    })]));

    let plan = planner.plan_intent("install vim").await.unwrap();
    let params = plan.steps()[0].params();
    assert_eq!(params["packages"][0], "vim");
}

// ---------------------------------------------------------------------------
// Error paths
// ---------------------------------------------------------------------------

#[tokio::test]
async fn provider_error_propagates() {
    let planner = make_planner(MockProvider::new([Err(ProviderError::Http {
        status: 500,
        body: "internal server error".into(),
    })]));

    assert!(matches!(
        planner.plan_intent("do something").await.unwrap_err(),
        PlanningError::Provider(_)
    ));
}

#[tokio::test]
async fn auth_error_propagates() {
    let planner = make_planner(MockProvider::new([Err(ProviderError::Auth(
        "invalid api key".into(),
    ))]));
    assert!(matches!(
        planner.plan_intent("do something").await.unwrap_err(),
        PlanningError::Provider(_)
    ));
}

#[tokio::test]
async fn end_turn_without_plan_returns_no_plan_proposed() {
    let planner = make_planner(MockProvider::new([end_turn_text(
        "I cannot help with that.",
    )]));
    assert_eq!(
        planner.plan_intent("do something").await.unwrap_err(),
        PlanningError::NoPlanProposed
    );
}

#[tokio::test]
async fn planner_stuck_after_max_turns() {
    // Provider returns get_system_state on every turn — never proposes a plan.
    let turns: Vec<_> = (0..6).map(|_| get_system_state_call()).collect();
    let planner = make_planner(MockProvider::new(turns));
    assert_eq!(
        planner.plan_intent("loop forever").await.unwrap_err(),
        PlanningError::PlannerStuck
    );
}

#[tokio::test]
async fn state_client_error_propagates() {
    let planner = make_planner_with_state(
        MockProvider::new([get_system_state_call()]),
        FailingStateClient {
            reason: "socket closed".into(),
        },
    );
    assert_eq!(
        planner.plan_intent("check state").await.unwrap_err(),
        PlanningError::StateUnavailable("socket closed".into())
    );
}

#[tokio::test]
async fn unknown_action_name_returns_invalid_plan_output() {
    let planner = make_planner(MockProvider::new([Ok(Completion {
        content: vec![ContentBlock::ToolUse {
            id: "tu_bad".into(),
            name: "propose_plan".into(),
            input: serde_json::json!({
                "summary": "bad plan",
                "explanation": "using a fake action",
                "steps": [{
                    "action_name": "RunShellCommand",
                    "summary": "run arbitrary shell",
                    "risk_level": "low",
                    "params": {}
                }]
            }),
        }],
        stop_reason: StopReason::ToolUse,
    })]));

    assert!(matches!(
        planner.plan_intent("run a command").await.unwrap_err(),
        PlanningError::InvalidPlanOutput(_)
    ));
}

#[tokio::test]
async fn empty_steps_array_returns_invalid_plan_output() {
    let planner = make_planner(MockProvider::new([Ok(Completion {
        content: vec![ContentBlock::ToolUse {
            id: "tu_empty".into(),
            name: "propose_plan".into(),
            input: serde_json::json!({
                "summary": "nothing to do",
                "explanation": "no steps",
                "steps": []
            }),
        }],
        stop_reason: StopReason::ToolUse,
    })]));

    assert!(matches!(
        planner.plan_intent("do nothing").await.unwrap_err(),
        PlanningError::InvalidPlanOutput(_)
    ));
}

// ---------------------------------------------------------------------------
// Error message stability — pin human-readable strings
// ---------------------------------------------------------------------------

#[test]
fn planning_error_messages_are_stable() {
    assert_eq!(
        PlanningError::EmptyIntent.to_string(),
        "intent must not be empty"
    );
    assert_eq!(
        PlanningError::StateUnavailable("disk timeout".into()).to_string(),
        "state unavailable: disk timeout"
    );
    assert_eq!(
        PlanningError::PlannerStuck.to_string(),
        "planner did not propose a plan within the allowed turns"
    );
    assert_eq!(
        PlanningError::NoPlanProposed.to_string(),
        "planner ended without proposing a plan"
    );
}
