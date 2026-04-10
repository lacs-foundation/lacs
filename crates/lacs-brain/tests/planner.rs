use lacs_brain::planner::Planner;
use lacs_brain::state_client::{CuratedState, StateClient};
use std::cell::RefCell;

#[derive(Default)]
struct MockStateClient {
    calls: RefCell<usize>,
}

impl StateClient for MockStateClient {
    fn curated_state(&self) -> Result<CuratedState, lacs_brain::planner::PlanningError> {
        *self.calls.borrow_mut() += 1;
        Ok(CuratedState {
            host_name: "silverblue".to_string(),
            deployment: "fedora/41".to_string(),
            services: vec!["NetworkManager.service".to_string()],
            flatpaks: vec!["org.mozilla.firefox".to_string()],
            toolboxes: vec!["lacs-dev".to_string()],
        })
    }
}

#[test]
fn planner_turns_update_intent_into_a_typed_plan() {
    let client = MockStateClient::default();
    let planner = Planner::new(client);

    let plan = planner
        .plan_intent("update this machine")
        .expect("plan should be created");

    assert_eq!(plan.intent, "update this machine");
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].action_name, "UpdateSystem");
    assert!(plan.steps[0].approval_required);
    assert_eq!(plan.steps[0].risk_level.as_str(), "high");
}

#[test]
fn planner_uses_curated_state_once_for_read_only_intents() {
    let client = MockStateClient::default();
    let planner = Planner::new(client);

    let plan = planner.plan_intent("show me the machine state").unwrap();

    assert_eq!(plan.steps[0].action_name, "GetSystemState");
    assert!(!plan.steps[0].approval_required);
    assert_eq!(plan.current_state.host_name, "silverblue");
    assert_eq!(planner.state_client_call_count(), 1);
}

#[test]
fn planner_keeps_mutating_steps_approval_gated() {
    let client = MockStateClient::default();
    let planner = Planner::new(client);

    let plan = planner
        .plan_intent("create a toolbox and install firefox")
        .unwrap();

    assert!(plan.steps.iter().all(|step| step.approval_required));
    assert!(plan
        .steps
        .iter()
        .any(|step| step.action_name == "CreateToolbox"));
    assert!(plan
        .steps
        .iter()
        .any(|step| step.action_name == "InstallFlatpak"));
}
