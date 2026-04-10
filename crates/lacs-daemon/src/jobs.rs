use lacs_types::JobState;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum JobError {
    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition { from: JobState, to: JobState },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobStateMachine {
    job_id: String,
    state: JobState,
}

impl JobStateMachine {
    pub fn new(job_id: impl Into<String>) -> Self {
        Self {
            job_id: job_id.into(),
            state: JobState::Queued,
        }
    }

    pub fn job_id(&self) -> &str {
        &self.job_id
    }

    pub fn state(&self) -> JobState {
        self.state.clone()
    }

    pub fn transition_to(&mut self, next: JobState) -> Result<(), JobError> {
        if allowed_transition(&self.state, &next) {
            self.state = next;
            Ok(())
        } else {
            Err(JobError::InvalidTransition {
                from: self.state.clone(),
                to: next,
            })
        }
    }
}

fn allowed_transition(current: &JobState, next: &JobState) -> bool {
    matches!(
        (current, next),
        (JobState::Queued, JobState::Running)
            | (JobState::Queued, JobState::Canceled)
            | (JobState::Running, JobState::Succeeded)
            | (JobState::Running, JobState::Failed)
            | (JobState::Running, JobState::Canceled)
            | (JobState::Running, JobState::RolledBack)
            | (JobState::Running, JobState::NeedsReboot)
    )
}
