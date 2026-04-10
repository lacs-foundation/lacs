use crate::planner::PlanningError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuratedState {
    pub host_name: String,
    pub deployment: String,
    pub services: Vec<String>,
    pub flatpaks: Vec<String>,
    pub toolboxes: Vec<String>,
}

impl CuratedState {
    /// Construct a `CuratedState` with non-empty `host_name` and `deployment`.
    /// Panics in debug builds if either is empty; callers should validate
    /// before construction rather than relying on runtime panics.
    pub fn new(
        host_name: impl Into<String>,
        deployment: impl Into<String>,
        services: Vec<String>,
        flatpaks: Vec<String>,
        toolboxes: Vec<String>,
    ) -> Self {
        let host_name = host_name.into();
        let deployment = deployment.into();
        debug_assert!(!host_name.is_empty(), "host_name must not be empty");
        debug_assert!(!deployment.is_empty(), "deployment must not be empty");
        Self { host_name, deployment, services, flatpaks, toolboxes }
    }
}

pub trait StateClient {
    fn curated_state(&self) -> Result<CuratedState, PlanningError>;
}
