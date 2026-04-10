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

pub trait StateClient {
    fn curated_state(&self) -> Result<CuratedState, PlanningError>;
}
