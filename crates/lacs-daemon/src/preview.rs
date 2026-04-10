use lacs_types::RiskLevel;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewDraft {
    pub request_hash: String,
    pub summary: String,
    pub risk_level: RiskLevel,
    pub reboot_required: bool,
    pub rollback_available: bool,
}

impl PreviewDraft {
    pub fn new(
        request_hash: impl Into<String>,
        summary: impl Into<String>,
        risk_level: RiskLevel,
    ) -> Self {
        Self {
            request_hash: request_hash.into(),
            summary: summary.into(),
            risk_level,
            reboot_required: false,
            rollback_available: true,
        }
    }
}

pub fn preview_matches_request(request_hash: &str, preview: &PreviewDraft) -> bool {
    !request_hash.is_empty() && preview.request_hash == request_hash
}
