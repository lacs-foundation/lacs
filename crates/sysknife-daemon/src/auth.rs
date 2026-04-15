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
}
