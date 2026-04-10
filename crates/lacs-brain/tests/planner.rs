use lacs_brain::planner::{PlanningError, Planner};
use lacs_brain::state_client::{CuratedState, StateClient};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

// ---------------------------------------------------------------------------
// Test clients
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct MockStateClient {
    call_count: Arc<AtomicUsize>,
}

impl StateClient for MockStateClient {
    fn curated_state(&self) -> Result<CuratedState, PlanningError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(CuratedState {
            host_name: "silverblue".to_string(),
            deployment: "fedora/41".to_string(),
            services: vec!["NetworkManager.service".to_string()],
            flatpaks: vec!["org.mozilla.firefox".to_string()],
            toolboxes: vec!["lacs-dev".to_string()],
        })
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
// Existing behaviour tests
// ---------------------------------------------------------------------------

#[test]
fn planner_turns_update_intent_into_a_typed_plan() {
    let planner = Planner::new(MockStateClient::default());

    let plan = planner
        .plan_intent("update this machine")
        .expect("plan should be created");

    assert_eq!(plan.intent(), "update this machine");
    assert_eq!(plan.steps().len(), 1);
    assert_eq!(plan.steps()[0].action_name(), "UpdateSystem");
    assert!(plan.steps()[0].approval_required());
    assert_eq!(plan.steps()[0].risk_level().as_str(), "high");
}

#[test]
fn planner_uses_curated_state_once_for_read_only_intents() {
    let client = MockStateClient::default();
    let call_count = client.call_count.clone();
    let planner = Planner::new(client);

    let plan = planner.plan_intent("show me the machine state").unwrap();

    assert_eq!(plan.steps()[0].action_name(), "GetSystemState");
    assert!(!plan.steps()[0].approval_required());
    assert_eq!(plan.current_state().host_name, "silverblue");
    assert_eq!(call_count.load(Ordering::Relaxed), 1);
}

#[test]
fn planner_keeps_mutating_steps_approval_gated() {
    let planner = Planner::new(MockStateClient::default());

    let plan = planner
        .plan_intent("create a toolbox and install firefox")
        .unwrap();

    assert!(plan.steps().iter().all(|step| step.approval_required()));
    assert!(plan
        .steps()
        .iter()
        .any(|step| step.action_name() == "CreateToolbox"));
    assert!(plan
        .steps()
        .iter()
        .any(|step| step.action_name() == "InstallFlatpak"));
}

// ---------------------------------------------------------------------------
// Error path tests
// ---------------------------------------------------------------------------

#[test]
fn planner_rejects_empty_intent() {
    let planner = Planner::new(MockStateClient::default());
    assert_eq!(
        planner.plan_intent("").unwrap_err(),
        PlanningError::EmptyIntent,
    );
}

#[test]
fn planner_rejects_whitespace_only_intent() {
    let planner = Planner::new(MockStateClient::default());
    assert_eq!(
        planner.plan_intent("   \t\n  ").unwrap_err(),
        PlanningError::EmptyIntent,
    );
}

#[test]
fn planner_does_not_call_state_client_on_empty_intent() {
    let client = MockStateClient::default();
    let call_count = client.call_count.clone();
    let planner = Planner::new(client);

    let _ = planner.plan_intent("");

    assert_eq!(
        call_count.load(Ordering::Relaxed),
        0,
        "state client must not be contacted when intent is empty"
    );
}

#[test]
fn planner_propagates_state_client_error() {
    let planner = Planner::new(FailingStateClient {
        reason: "disk timeout".to_string(),
    });

    let err = planner
        .plan_intent("update this machine")
        .unwrap_err();

    assert_eq!(
        err,
        PlanningError::StateUnavailable("disk timeout".to_string()),
    );
}

// ---------------------------------------------------------------------------
// Error message stability tests — pin the human-readable strings so changes
// to Display impls produce a visible test failure rather than a silent drift.
// ---------------------------------------------------------------------------

#[test]
fn planning_error_messages_are_stable() {
    assert_eq!(
        PlanningError::EmptyIntent.to_string(),
        "intent must not be empty",
    );
    assert_eq!(
        PlanningError::StateUnavailable("disk timeout".to_string()).to_string(),
        "state unavailable: disk timeout",
    );
}
