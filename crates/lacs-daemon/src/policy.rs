pub fn approval_matches_request(request_hash: &str, approval_hash: &str) -> bool {
    !request_hash.is_empty() && request_hash == approval_hash
}
