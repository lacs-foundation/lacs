#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalBinding {
    pub request_hash: String,
    pub approval_hash: String,
}

pub fn approval_matches_request(request_hash: &str, approval_hash: &str) -> bool {
    !request_hash.is_empty() && request_hash == approval_hash
}

impl ApprovalBinding {
    pub fn is_valid(&self) -> bool {
        approval_matches_request(&self.request_hash, &self.approval_hash)
    }
}
