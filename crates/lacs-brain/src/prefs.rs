//! User preference file operations.
//!
//! Preferences are stored as a flat markdown file (default
//! `~/.config/lacs/prefs.md`, respects `XDG_CONFIG_HOME`). The path is
//! provided by the caller. Each preference is a single line prefixed with
//! `- `. The file is read by the planner at the start of each `plan_intent()`
//! call and injected into the system prompt.

use std::io;
use std::path::Path;

/// Maximum size of the preferences file in bytes. Prevents runaway growth
/// from a misbehaving LLM that calls `remember` in a loop. 10 KB is roughly
/// 200 preferences — well beyond any practical use.
pub const PREFS_MAX_BYTES: u64 = 10_240;

/// Substrings that indicate sensitive data. If any of these appear
/// (case-insensitive) in a preference, it is rejected.
const SENSITIVE_PATTERNS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "private_key",
    "token",
    "credential",
    "-----begin",
];

/// String prefixes that indicate well-known secret formats (API keys, access tokens, PATs).
const SENSITIVE_PREFIXES: &[&str] = &[
    "sk-",         // Anthropic / OpenAI
    "ghp_",        // GitHub personal access token
    "github_pat_", // GitHub fine-grained PAT
    "gho_",        // GitHub OAuth token
    "xoxb-",       // Slack bot token
    "xoxp-",       // Slack user token
];

/// Read the user preferences file. Returns `Ok(None)` if the file does not
/// exist or is empty; returns `Ok(Some(content))` on success; propagates I/O
/// errors other than `NotFound`.
pub fn read_prefs(path: &Path) -> Result<Option<String>, io::Error> {
    match std::fs::read_to_string(path) {
        Ok(content) if content.trim().is_empty() => Ok(None),
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn append_pref(path: &Path, fact: &str) -> Result<(), io::Error> {
    // Reject facts containing newlines — they would corrupt the file format
    // and could bypass the sensitive-data filter.
    if fact.contains('\n') || fact.contains('\r') {
        return Err(io::Error::other("preference must be a single line"));
    }

    // Create parent directories if needed.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Single read: check size, dedup, and build combined content.
    let existing = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };

    if existing.len() as u64 >= PREFS_MAX_BYTES {
        return Err(io::Error::other(format!(
            "preferences file exceeds size limit ({} bytes); \
             remove unused preferences before adding new ones",
            PREFS_MAX_BYTES
        )));
    }

    // Check for duplicates.
    if existing.lines().any(|line| {
        line.strip_prefix("- ")
            .is_some_and(|stripped| stripped == fact)
    }) {
        return Ok(()); // Already present, no-op.
    }

    let new_line = format!("- {fact}\n");
    let combined = format!("{existing}{new_line}");

    // Write via temp-file + rename for crash safety (not concurrency-safe).
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, combined.as_bytes())?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn remove_pref(path: &Path, fact: &str) -> Result<bool, io::Error> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };

    let target = format!("- {fact}");
    let mut found = false;
    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| {
            if *line == target {
                found = true;
                false
            } else {
                true
            }
        })
        .collect();

    if !found {
        return Ok(false);
    }

    let new_content = if filtered.is_empty() {
        String::new()
    } else {
        filtered.join("\n") + "\n"
    };

    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, new_content.as_bytes())?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(true)
}

pub fn contains_sensitive(fact: &str) -> bool {
    let lower = fact.to_lowercase();
    if SENSITIVE_PATTERNS.iter().any(|p| lower.contains(p)) {
        return true;
    }
    SENSITIVE_PREFIXES.iter().any(|p| fact.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn read_prefs_returns_none_when_file_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        assert!(read_prefs(&path).unwrap().is_none());
    }

    #[test]
    fn read_prefs_returns_none_when_file_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        std::fs::write(&path, "").unwrap();
        assert!(read_prefs(&path).unwrap().is_none());
    }

    #[test]
    fn append_pref_creates_file_and_writes_entry() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        append_pref(&path, "prefer vim-enhanced over vim").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "- prefer vim-enhanced over vim\n");
    }

    #[test]
    fn append_pref_appends_to_existing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        append_pref(&path, "first preference").unwrap();
        append_pref(&path, "second preference").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "- first preference\n- second preference\n");
    }

    #[test]
    fn append_pref_rejects_when_file_exceeds_size_limit() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        // Write a file that is just under the limit.
        let big_content = "- ".to_string() + &"x".repeat(PREFS_MAX_BYTES as usize - 3) + "\n";
        std::fs::write(&path, &big_content).unwrap();
        let result = append_pref(&path, "one more");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("size limit"));
    }

    #[test]
    fn append_pref_deduplicates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        append_pref(&path, "prefer vim-enhanced").unwrap();
        append_pref(&path, "prefer vim-enhanced").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("vim-enhanced").count(), 1);
    }

    #[test]
    fn remove_pref_removes_matching_line() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        append_pref(&path, "first pref").unwrap();
        append_pref(&path, "second pref").unwrap();
        let removed = remove_pref(&path, "first pref").unwrap();
        assert!(removed);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("first pref"));
        assert!(content.contains("second pref"));
    }

    #[test]
    fn remove_pref_returns_false_when_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        append_pref(&path, "some pref").unwrap();
        let removed = remove_pref(&path, "nonexistent").unwrap();
        assert!(!removed);
    }

    #[test]
    fn remove_pref_returns_false_when_file_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        let removed = remove_pref(&path, "anything").unwrap();
        assert!(!removed);
    }

    #[test]
    fn contains_sensitive_detects_password() {
        assert!(contains_sensitive("my password is hunter2"));
        assert!(contains_sensitive("ANTHROPIC_API_KEY=sk-abc123"));
    }

    #[test]
    fn contains_sensitive_detects_key_prefixes() {
        assert!(contains_sensitive("use key sk-ant-abc123 for anthropic"));
        assert!(contains_sensitive("github token ghp_abcdef1234567890"));
    }

    #[test]
    fn contains_sensitive_allows_normal_preferences() {
        assert!(!contains_sensitive("prefer vim-enhanced over vim"));
        assert!(!contains_sensitive("always use flathub remote"));
        assert!(!contains_sensitive(
            "skip large downloads on metered connections"
        ));
    }

    #[test]
    fn append_pref_rejects_newlines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        let result = append_pref(&path, "innocent\nsk-secret-key");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("single line"));
        // File should not have been created.
        assert!(!path.exists());
    }

    #[test]
    fn append_pref_rejects_carriage_returns() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        let result = append_pref(&path, "line one\rline two");
        assert!(result.is_err());
    }
}
