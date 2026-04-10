#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("stale approval")]
pub enum ApprovalError {
    StaleApproval {
        request_hash: String,
        approval_hash: String,
    },
}

pub fn approval_matches_request(request_hash: &str, approval_hash: &str) -> bool {
    !request_hash.is_empty() && request_hash == approval_hash
}

pub fn require_fresh_approval(
    request_hash: &str,
    approval_hash: &str,
) -> Result<(), ApprovalError> {
    if approval_matches_request(request_hash, approval_hash) {
        Ok(())
    } else {
        Err(ApprovalError::StaleApproval {
            request_hash: request_hash.to_string(),
            approval_hash: approval_hash.to_string(),
        })
    }
}
