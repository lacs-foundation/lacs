use lacs_brain::planner::PlanRiskLevel;

/// All CLI error categories with their exit-code mapping.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("plan rejected by user")]
    Rejected,

    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    #[error("planning failed: {0}")]
    PlanningFailed(String),

    #[error("config/daemon error: {0}")]
    ConfigOrDaemon(String),

    #[error("plan contains a {} step, but --max-risk ceiling is {}", .highest.as_str(), .ceiling.as_str())]
    RiskCeilingExceeded {
        highest: PlanRiskLevel,
        ceiling: PlanRiskLevel,
    },
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Rejected | Self::RiskCeilingExceeded { .. } => 1,
            Self::ExecutionFailed(_) => 2,
            Self::PlanningFailed(_) => 3,
            Self::ConfigOrDaemon(_) => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_rejected_is_1() {
        assert_eq!(CliError::Rejected.exit_code(), 1);
    }

    #[test]
    fn exit_code_risk_ceiling_exceeded_is_1() {
        assert_eq!(
            CliError::RiskCeilingExceeded {
                highest: PlanRiskLevel::High,
                ceiling: PlanRiskLevel::Medium,
            }
            .exit_code(),
            1
        );
    }

    #[test]
    fn exit_code_execution_failed_is_2() {
        assert_eq!(CliError::ExecutionFailed("boom".into()).exit_code(), 2);
    }

    #[test]
    fn exit_code_planning_failed_is_3() {
        assert_eq!(CliError::PlanningFailed("bad".into()).exit_code(), 3);
    }

    #[test]
    fn exit_code_config_or_daemon_is_4() {
        assert_eq!(CliError::ConfigOrDaemon("nope".into()).exit_code(), 4);
    }
}
