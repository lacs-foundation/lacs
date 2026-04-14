//! Approval policy engine — pure logic, no I/O.
//!
//! Determines whether plan steps can be auto-approved, need a human prompt,
//! or must be rejected outright based on CLI flags.
//!
//! Security invariant: `--yes` NEVER auto-approves HIGH risk steps regardless
//! of any flag combination. This is hardcoded, not configurable.

use lacs_brain::planner::{Plan, PlanRiskLevel};

/// The maximum risk level that `--yes` can auto-approve.
/// `--max-risk high` with `--yes` still only auto-approves up to MEDIUM.
/// HIGH always requires a human in the loop.
const HARDCODED_MAX_AUTO_APPROVE: MaxRisk = MaxRisk::Medium;

/// CLI risk-level argument (mirrors clap value-enum).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaxRisk {
    Low,
    Medium,
    High,
}

impl MaxRisk {
    fn includes(&self, risk: &PlanRiskLevel) -> bool {
        match self {
            MaxRisk::Low => matches!(risk, PlanRiskLevel::Low),
            MaxRisk::Medium => matches!(risk, PlanRiskLevel::Low | PlanRiskLevel::Medium),
            MaxRisk::High => true,
        }
    }
}

/// Flags that control approval behavior.
pub struct ApprovalPolicy {
    pub yes: bool,
    pub max_risk: Option<MaxRisk>,
    pub non_interactive: bool,
    pub dry_run: bool,
}

/// The result of evaluating a step or plan against the policy.
#[derive(Debug, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// No human input needed — auto-approved by policy.
    AutoApproved,
    /// Human must confirm before execution.
    RequiresPrompt,
    /// Non-interactive mode and this step would need a prompt — fail immediately.
    RequiresInteraction,
    /// Plan exceeds the --max-risk ceiling — abort.
    ExceedsCeiling,
}

impl ApprovalPolicy {
    /// Effective auto-approve ceiling, clamped by the hardcoded HIGH block.
    ///
    /// Returns `None` if `--yes` was not passed (no auto-approval at all).
    /// Returns `Some(level)` indicating the max risk that will be auto-approved.
    pub fn effective_auto_ceiling(&self) -> Option<MaxRisk> {
        if !self.yes {
            return None;
        }
        // User-requested ceiling (default to Low when --yes is bare).
        let requested = self.max_risk.unwrap_or(MaxRisk::Low);

        // Clamp: never auto-approve above HARDCODED_MAX_AUTO_APPROVE.
        let clamped = match (requested, HARDCODED_MAX_AUTO_APPROVE) {
            (MaxRisk::High, _) => HARDCODED_MAX_AUTO_APPROVE,
            (level, _) => level,
        };
        Some(clamped)
    }

    /// Decision for a single step's risk level.
    pub fn decide_step(&self, risk: &PlanRiskLevel) -> ApprovalDecision {
        // --dry-run: nothing executes, so everything is "approved" vacuously.
        if self.dry_run {
            return ApprovalDecision::AutoApproved;
        }

        // Check --max-risk ceiling (hard abort, independent of --yes).
        if let Some(ceiling) = self.max_risk {
            if !ceiling.includes(risk) {
                return ApprovalDecision::ExceedsCeiling;
            }
        }

        // Check auto-approval via --yes.
        if let Some(auto_ceiling) = self.effective_auto_ceiling() {
            if auto_ceiling.includes(risk) {
                return ApprovalDecision::AutoApproved;
            }
        }

        // Step needs a human prompt — can we do that?
        if self.non_interactive {
            return ApprovalDecision::RequiresInteraction;
        }

        ApprovalDecision::RequiresPrompt
    }

    /// Decision for the whole plan (uses the highest risk across all steps).
    ///
    /// Returns `AutoApproved` only if every step is auto-approved.
    /// Returns `ExceedsCeiling` if any step exceeds the ceiling.
    /// Returns `RequiresInteraction` if any step needs a prompt and we're non-interactive.
    /// Returns `RequiresPrompt` if at least one step needs human confirmation.
    pub fn decide_plan(&self, plan: &Plan) -> ApprovalDecision {
        let mut worst = ApprovalDecision::AutoApproved;
        for step in plan.steps() {
            let d = self.decide_step(step.risk_level());
            match d {
                // These are terminal — return immediately.
                ApprovalDecision::ExceedsCeiling => return d,
                ApprovalDecision::RequiresInteraction => return d,
                // Escalate: RequiresPrompt > AutoApproved.
                ApprovalDecision::RequiresPrompt => worst = ApprovalDecision::RequiresPrompt,
                ApprovalDecision::AutoApproved => {}
            }
        }
        worst
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lacs_brain::action_name::ActionName;
    use lacs_brain::planner::{Plan, PlanStep};

    fn step(risk: PlanRiskLevel) -> PlanStep {
        PlanStep::new(
            ActionName::parse("GetDiskUsage").unwrap(),
            "test step".into(),
            risk,
            serde_json::json!({}),
        )
        .unwrap()
    }

    fn plan(risks: &[PlanRiskLevel]) -> Plan {
        let steps: Vec<PlanStep> = risks.iter().map(|r| step(r.clone())).collect();
        Plan::new(
            "test".into(),
            "test plan".into(),
            "test explanation".into(),
            steps,
        )
        .unwrap()
    }

    fn policy(yes: bool, max_risk: Option<MaxRisk>, non_interactive: bool, dry_run: bool) -> ApprovalPolicy {
        ApprovalPolicy { yes, max_risk, non_interactive, dry_run }
    }

    // --- Phase A: Security policy tests ---

    // 1. --yes alone never approves Medium
    #[test]
    fn yes_alone_requires_prompt_for_medium() {
        let p = policy(true, None, false, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::Medium), ApprovalDecision::RequiresPrompt);
    }

    // 2. --yes alone never approves High
    #[test]
    fn yes_alone_requires_prompt_for_high() {
        let p = policy(true, None, false, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::High), ApprovalDecision::RequiresPrompt);
    }

    // --yes alone auto-approves Low
    #[test]
    fn yes_alone_auto_approves_low() {
        let p = policy(true, None, false, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::Low), ApprovalDecision::AutoApproved);
    }

    // 3. --yes --max-risk medium approves Medium
    #[test]
    fn yes_max_risk_medium_auto_approves_medium() {
        let p = policy(true, Some(MaxRisk::Medium), false, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::Medium), ApprovalDecision::AutoApproved);
    }

    // 4. --yes --max-risk high does NOT auto-approve High (hardcoded)
    #[test]
    fn yes_max_risk_high_does_not_auto_approve_high() {
        let p = policy(true, Some(MaxRisk::High), false, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::High), ApprovalDecision::RequiresPrompt);
    }

    // --yes --max-risk high auto-approves Medium (ceiling clamps to Medium)
    #[test]
    fn yes_max_risk_high_auto_approves_medium() {
        let p = policy(true, Some(MaxRisk::High), false, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::Medium), ApprovalDecision::AutoApproved);
    }

    // 5. --max-risk medium with High step → ExceedsCeiling
    #[test]
    fn max_risk_ceiling_exceeds_for_high_step() {
        let p = policy(false, Some(MaxRisk::Medium), false, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::High), ApprovalDecision::ExceedsCeiling);
    }

    // --max-risk low with Medium step → ExceedsCeiling
    #[test]
    fn max_risk_low_ceiling_exceeds_for_medium_step() {
        let p = policy(false, Some(MaxRisk::Low), false, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::Medium), ApprovalDecision::ExceedsCeiling);
    }

    // 6. --non-interactive with Medium step → RequiresInteraction
    #[test]
    fn non_interactive_no_yes_requires_interaction_for_medium() {
        let p = policy(false, None, true, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::Medium), ApprovalDecision::RequiresInteraction);
    }

    // 7. --non-interactive --yes with Low → AutoApproved
    #[test]
    fn non_interactive_yes_auto_approves_low() {
        let p = policy(true, None, true, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::Low), ApprovalDecision::AutoApproved);
    }

    // --non-interactive --yes with Medium → RequiresInteraction (ceiling is Low)
    #[test]
    fn non_interactive_yes_requires_interaction_for_medium() {
        let p = policy(true, None, true, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::Medium), ApprovalDecision::RequiresInteraction);
    }

    // 8. --dry-run with any risk → AutoApproved
    #[test]
    fn dry_run_auto_approves_low() {
        let p = policy(false, None, false, true);
        assert_eq!(p.decide_step(&PlanRiskLevel::Low), ApprovalDecision::AutoApproved);
    }

    #[test]
    fn dry_run_auto_approves_medium() {
        let p = policy(false, None, false, true);
        assert_eq!(p.decide_step(&PlanRiskLevel::Medium), ApprovalDecision::AutoApproved);
    }

    #[test]
    fn dry_run_auto_approves_high() {
        let p = policy(false, None, false, true);
        assert_eq!(p.decide_step(&PlanRiskLevel::High), ApprovalDecision::AutoApproved);
    }

    // --- Plan-level decisions ---

    // All-Low plan with --yes → AutoApproved
    #[test]
    fn plan_all_low_with_yes_auto_approved() {
        let p = policy(true, None, false, false);
        let pl = plan(&[PlanRiskLevel::Low, PlanRiskLevel::Low]);
        assert_eq!(p.decide_plan(&pl), ApprovalDecision::AutoApproved);
    }

    // Plan with one Medium step, --yes → RequiresPrompt
    #[test]
    fn plan_mixed_low_medium_with_yes_requires_prompt() {
        let p = policy(true, None, false, false);
        let pl = plan(&[PlanRiskLevel::Low, PlanRiskLevel::Medium]);
        assert_eq!(p.decide_plan(&pl), ApprovalDecision::RequiresPrompt);
    }

    // Plan with High step, --max-risk medium → ExceedsCeiling
    #[test]
    fn plan_high_step_with_max_risk_medium_exceeds_ceiling() {
        let p = policy(true, Some(MaxRisk::Medium), false, false);
        let pl = plan(&[PlanRiskLevel::Low, PlanRiskLevel::High]);
        assert_eq!(p.decide_plan(&pl), ApprovalDecision::ExceedsCeiling);
    }

    // Plan with Medium step, --non-interactive --yes → RequiresInteraction
    #[test]
    fn plan_medium_step_non_interactive_yes_requires_interaction() {
        let p = policy(true, None, true, false);
        let pl = plan(&[PlanRiskLevel::Low, PlanRiskLevel::Medium]);
        assert_eq!(p.decide_plan(&pl), ApprovalDecision::RequiresInteraction);
    }

    // No flags at all — Low step still RequiresPrompt (no --yes)
    #[test]
    fn no_flags_low_step_requires_prompt() {
        let p = policy(false, None, false, false);
        assert_eq!(p.decide_step(&PlanRiskLevel::Low), ApprovalDecision::RequiresPrompt);
    }

    // effective_auto_ceiling tests
    #[test]
    fn effective_ceiling_none_without_yes() {
        let p = policy(false, None, false, false);
        assert!(p.effective_auto_ceiling().is_none());
    }

    #[test]
    fn effective_ceiling_low_with_bare_yes() {
        let p = policy(true, None, false, false);
        assert_eq!(p.effective_auto_ceiling(), Some(MaxRisk::Low));
    }

    #[test]
    fn effective_ceiling_medium_with_yes_max_risk_medium() {
        let p = policy(true, Some(MaxRisk::Medium), false, false);
        assert_eq!(p.effective_auto_ceiling(), Some(MaxRisk::Medium));
    }

    #[test]
    fn effective_ceiling_clamped_to_medium_with_yes_max_risk_high() {
        let p = policy(true, Some(MaxRisk::High), false, false);
        assert_eq!(p.effective_auto_ceiling(), Some(MaxRisk::Medium));
    }
}
