use lacs_types::CallerRole;

pub const OBSERVER_GROUP: &str = "lacs-observer";
pub const DEV_GROUP: &str = "lacs-dev";
pub const ADMIN_GROUP: &str = "lacs-admin";
pub const BOOT_GROUP: &str = "lacs-boot";
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

fn role_rank(role: &CallerRole) -> u8 {
    match role {
        CallerRole::Observer => 0,
        CallerRole::Dev => 1,
        CallerRole::Admin => 2,
        CallerRole::Boot => 3,
    }
}
