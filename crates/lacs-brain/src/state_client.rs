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
    ///
    /// # Panics
    /// Panics if `host_name` or `deployment` is empty — these are programmer
    /// errors; callers must validate before construction.
    pub fn new(
        host_name: impl Into<String>,
        deployment: impl Into<String>,
        services: Vec<String>,
        flatpaks: Vec<String>,
        toolboxes: Vec<String>,
    ) -> Self {
        let host_name = host_name.into();
        let deployment = deployment.into();
        assert!(!host_name.is_empty(), "host_name must not be empty");
        assert!(!deployment.is_empty(), "deployment must not be empty");
        Self {
            host_name,
            deployment,
            services,
            flatpaks,
            toolboxes,
        }
    }
}

pub trait StateClient: Send + Sync {
    /// Return the curated system state for LLM consumption.
    ///
    /// Implementors should return `Err(PlanningError::StateUnavailable(_))`
    /// when the daemon is unreachable or the state cannot be read. Other
    /// `PlanningError` variants are semantically incorrect here.
    fn curated_state(&self) -> Result<CuratedState, PlanningError>;
}
