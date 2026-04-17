use sysknife_types::CallerRole;

pub const OBSERVER_GROUP: &str = "sysknife-observer";
pub const DEV_GROUP: &str = "sysknife-dev";
pub const ADMIN_GROUP: &str = "sysknife-admin";
pub const BOOT_GROUP: &str = "sysknife-boot";
pub const WHEEL_GROUP: &str = "wheel";

pub fn highest_role_from_groups<I, S>(groups: I) -> CallerRole
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    groups
        .into_iter()
        .map(|group| role_for_group(group.as_ref()))
        .fold(CallerRole::Observer, higher_role)
}

fn role_for_group(group: &str) -> CallerRole {
    match group {
        BOOT_GROUP => CallerRole::Boot,
        ADMIN_GROUP | WHEEL_GROUP => CallerRole::Admin,
        DEV_GROUP => CallerRole::Dev,
        OBSERVER_GROUP => CallerRole::Observer,
        _ => CallerRole::Observer,
    }
}

fn higher_role(current: CallerRole, candidate: CallerRole) -> CallerRole {
    if role_rank(&candidate) > role_rank(&current) {
        candidate
    } else {
        current
    }
}

pub(crate) fn role_rank(role: &CallerRole) -> u8 {
    match role {
        CallerRole::Observer => 0,
        CallerRole::Dev => 1,
        CallerRole::Admin => 2,
        CallerRole::Boot => 3,
    }
}

// ---------------------------------------------------------------------------
// Token authentication (vsock connections)
// ---------------------------------------------------------------------------

/// Validate `presented_token` against the token stored in `token_path`.
///
/// Returns the role the token holder is granted (read from the
/// `SYSKNIFE_TOKEN_ROLE` env var, defaulting to `Dev`) on success, or `None`
/// if the token file is absent, unreadable, or the token does not match.
///
/// Whitespace (including trailing newlines written by `echo`) is stripped from
/// the stored token before comparison, so `echo TOKEN > ~/.config/sysknife/token`
/// works without modification.
pub fn validate_token_against_file(
    presented_token: &str,
    token_path: &std::path::Path,
) -> Option<CallerRole> {
    if presented_token.is_empty() {
        return None;
    }
    let stored = match std::fs::read_to_string(token_path) {
        Ok(s) => s,
        Err(_) => return None,
    };
    let stored = stored.trim();
    if stored.is_empty() || stored != presented_token {
        return None;
    }
    Some(token_role())
}

/// Return the `CallerRole` granted to token-authenticated vsock connections.
///
/// Reads `SYSKNIFE_TOKEN_ROLE` env var; defaults to `Dev`. Invalid values
/// fall back to `Dev` with a warning.
pub fn token_role() -> CallerRole {
    match std::env::var("SYSKNIFE_TOKEN_ROLE")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "observer" => CallerRole::Observer,
        "admin" => CallerRole::Admin,
        "boot" => CallerRole::Boot,
        "dev" | "" => CallerRole::Dev,
        other => {
            eprintln!(
                "[sysknife-daemon] WARNING: unknown SYSKNIFE_TOKEN_ROLE={other:?}; \
                 defaulting to Dev"
            );
            CallerRole::Dev
        }
    }
}

/// Default path for the daemon token file.
pub fn default_token_path() -> std::path::PathBuf {
    sysknife_core::config::prefs_path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/tmp"))
        .join("token")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn role(groups: &[&str]) -> CallerRole {
        highest_role_from_groups(groups.iter().copied())
    }

    #[test]
    fn empty_groups_resolves_to_observer() {
        assert_eq!(role(&[]), CallerRole::Observer);
    }

    #[test]
    fn unknown_group_resolves_to_observer() {
        assert_eq!(role(&["plugdev", "dialout"]), CallerRole::Observer);
    }

    #[test]
    fn lacs_observer_group_resolves_to_observer() {
        assert_eq!(role(&[OBSERVER_GROUP]), CallerRole::Observer);
    }

    #[test]
    fn lacs_dev_group_resolves_to_dev() {
        assert_eq!(role(&[DEV_GROUP]), CallerRole::Dev);
    }

    #[test]
    fn lacs_admin_group_resolves_to_admin() {
        assert_eq!(role(&[ADMIN_GROUP]), CallerRole::Admin);
    }

    #[test]
    fn wheel_group_resolves_to_admin() {
        assert_eq!(role(&[WHEEL_GROUP]), CallerRole::Admin);
    }

    #[test]
    fn lacs_boot_group_resolves_to_boot() {
        assert_eq!(role(&[BOOT_GROUP]), CallerRole::Boot);
    }

    #[test]
    fn highest_role_wins_when_multiple_groups_present() {
        // A user in both sysknife-dev and wheel gets Admin (wheel > Dev).
        assert_eq!(role(&[DEV_GROUP, WHEEL_GROUP]), CallerRole::Admin);
    }

    #[test]
    fn boot_role_beats_admin_and_wheel() {
        assert_eq!(
            role(&[BOOT_GROUP, ADMIN_GROUP, WHEEL_GROUP]),
            CallerRole::Boot
        );
    }

    #[test]
    fn mixed_known_and_unknown_groups_returns_highest_known() {
        assert_eq!(role(&["plugdev", DEV_GROUP, "audio"]), CallerRole::Dev);
    }

    // --- token auth ---

    #[test]
    fn valid_token_matches_and_returns_dev_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "secret123").unwrap();
        assert_eq!(
            validate_token_against_file("secret123", &path),
            Some(CallerRole::Dev)
        );
    }

    #[test]
    fn token_file_with_trailing_newline_still_matches() {
        // `echo TOKEN > file` appends a newline — must still work.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "secret123\n").unwrap();
        assert_eq!(
            validate_token_against_file("secret123", &path),
            Some(CallerRole::Dev)
        );
    }

    #[test]
    fn wrong_token_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "correct\n").unwrap();
        assert_eq!(validate_token_against_file("wrong", &path), None);
    }

    #[test]
    fn absent_token_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent");
        assert_eq!(validate_token_against_file("any", &path), None);
    }

    #[test]
    fn empty_presented_token_is_always_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "").unwrap();
        assert_eq!(validate_token_against_file("", &path), None);
    }

    #[test]
    fn empty_stored_token_is_rejected_even_with_matching_presented() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "\n").unwrap();
        assert_eq!(validate_token_against_file("", &path), None);
    }
}
