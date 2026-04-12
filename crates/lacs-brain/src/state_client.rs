use crate::planner::PlanningError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CuratedState {
    host_name: String,
    deployment: String,
    services: Vec<String>,
    flatpaks: Vec<String>,
    toolboxes: Vec<String>,
    layered_packages: Vec<String>,
    containers: Vec<String>,
    users: Vec<String>,
}

impl CuratedState {
    /// Construct a `CuratedState` with a non-empty `host_name`.
    ///
    /// `deployment` may be empty on non-ostree systems where `rpm-ostree`
    /// is not available.
    ///
    /// Returns `Err` if `host_name` is empty.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host_name: impl Into<String>,
        deployment: impl Into<String>,
        services: Vec<String>,
        flatpaks: Vec<String>,
        toolboxes: Vec<String>,
        layered_packages: Vec<String>,
        containers: Vec<String>,
        users: Vec<String>,
    ) -> Result<Self, String> {
        let host_name = host_name.into();
        let deployment = deployment.into();
        if host_name.is_empty() {
            return Err("host_name must not be empty".into());
        }
        Ok(Self {
            host_name,
            deployment,
            services,
            flatpaks,
            toolboxes,
            layered_packages,
            containers,
            users,
        })
    }

    pub fn host_name(&self) -> &str {
        &self.host_name
    }

    pub fn deployment(&self) -> &str {
        &self.deployment
    }

    pub fn services(&self) -> &[String] {
        &self.services
    }

    pub fn flatpaks(&self) -> &[String] {
        &self.flatpaks
    }

    pub fn toolboxes(&self) -> &[String] {
        &self.toolboxes
    }

    pub fn layered_packages(&self) -> &[String] {
        &self.layered_packages
    }

    pub fn containers(&self) -> &[String] {
        &self.containers
    }

    pub fn users(&self) -> &[String] {
        &self.users
    }
}

/// Custom `Deserialize` that routes through `CuratedState::new` so invariants
/// (non-empty host_name) are enforced at deserialization time.
impl<'de> Deserialize<'de> for CuratedState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            host_name: String,
            deployment: String,
            services: Vec<String>,
            flatpaks: Vec<String>,
            toolboxes: Vec<String>,
            #[serde(default)]
            layered_packages: Vec<String>,
            #[serde(default)]
            containers: Vec<String>,
            #[serde(default)]
            users: Vec<String>,
        }

        let raw = Raw::deserialize(deserializer)?;
        CuratedState::new(
            raw.host_name,
            raw.deployment,
            raw.services,
            raw.flatpaks,
            raw.toolboxes,
            raw.layered_packages,
            raw.containers,
            raw.users,
        )
        .map_err(serde::de::Error::custom)
    }
}

pub trait StateClient: Send + Sync {
    /// Return the curated system state for LLM consumption.
    ///
    /// Implementors should return `Err(PlanningError::StateUnavailable(_))`
    /// when the daemon is unreachable or the state cannot be read. Other
    /// `PlanningError` variants are semantically incorrect here.
    fn curated_state(&self) -> Result<CuratedState, PlanningError>;

    /// Run a read-only action on the daemon and return its stdout.
    ///
    /// Only Low-risk (Observer-level) actions are allowed. The daemon
    /// enforces this constraint; callers need not pre-filter.
    fn query_action(
        &self,
        action_name: &str,
        params: &serde_json::Value,
    ) -> Result<String, PlanningError>;
}
