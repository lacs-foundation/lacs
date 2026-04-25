use super::{command_mechanism, ActionMechanism, ActionSpec};
use sysknife_types::RiskLevel;

pub fn specs() -> Vec<ActionSpec> {
    vec![
        install_flatpak("testuser", "app-id", "flathub"),
        remove_flatpak("testuser", "app-id"),
        search_flatpak_apps("search-term"),
        list_flatpak_remotes("testuser"),
        list_installed_flatpaks("testuser"),
        add_flatpak_remote("testuser", "remote", "https://example.invalid"),
        remove_flatpak_remote("testuser", "remote"),
        get_flatpak_app_info("testuser", "app-id"),
        update_flatpak("testuser", Some("com.example.App")),
    ]
}

/// Run a Flatpak command as the target user via `sudo runuser -u user -- flatpak <argv>`.
///
/// Flatpak user installations live under `~/.local/share/flatpak/` and are
/// accessed through the user's D-Bus session. The daemon runs as `sysknife`
/// (a system user) with no user installation; `runuser -u` switches to the
/// correct user UID without spawning a login shell, so each argv element is
/// passed to `flatpak` verbatim.
///
/// **Shell-injection safety:** unlike `runuser -l user -c "<shell-string>"`,
/// the `-u user -- argv` form bypasses the shell entirely. There is no string
/// interpolation, no metacharacter expansion, and no quoting concern — every
/// argument reaches `flatpak(1)` exactly as supplied. Callers must still pass
/// arguments through `validated_safe_arg`/`validated_username` upstream so a
/// hostile value cannot impersonate a flag (`-X`) or break out of the
/// command's own option parser, but they no longer have to defend against
/// shell metacharacters.
fn flatpak_as(username: &str, args: &[&str]) -> ActionMechanism {
    let mut argv: Vec<String> = vec![
        "runuser".to_string(),
        "-u".to_string(),
        username.to_string(),
        "--".to_string(),
        "flatpak".to_string(),
    ];
    argv.extend(args.iter().map(|s| s.to_string()));
    ActionMechanism::Command {
        program: "sudo",
        args: argv,
    }
}

pub fn install_flatpak(username: &str, app_id: &str, remote: &str) -> ActionSpec {
    ActionSpec {
        action_name: "InstallFlatpak",
        mechanism: flatpak_as(username, &["install", "--user", "-y", remote, app_id]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn remove_flatpak(username: &str, app_id: &str) -> ActionSpec {
    ActionSpec {
        action_name: "RemoveFlatpak",
        mechanism: flatpak_as(username, &["uninstall", "--user", "-y", app_id]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

/// Search is system-wide (no user context needed) — the Flatpak repo index
/// is shared and does not require a D-Bus session or user installation.
pub fn search_flatpak_apps(term: &str) -> ActionSpec {
    ActionSpec {
        action_name: "SearchFlatpakApps",
        mechanism: command_mechanism("flatpak", ["search", term]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn list_flatpak_remotes(username: &str) -> ActionSpec {
    ActionSpec {
        action_name: "ListFlatpakRemotes",
        mechanism: flatpak_as(username, &["remotes", "--user", "--columns=name,url"]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn add_flatpak_remote(username: &str, remote: &str, url: &str) -> ActionSpec {
    ActionSpec {
        action_name: "AddFlatpakRemote",
        mechanism: flatpak_as(
            username,
            &["remote-add", "--user", "--if-not-exists", remote, url],
        ),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn remove_flatpak_remote(username: &str, remote: &str) -> ActionSpec {
    ActionSpec {
        action_name: "RemoveFlatpakRemote",
        mechanism: flatpak_as(username, &["remote-delete", "--user", remote]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn list_installed_flatpaks(username: &str) -> ActionSpec {
    ActionSpec {
        action_name: "ListInstalledFlatpaks",
        mechanism: flatpak_as(
            username,
            &[
                "list",
                "--user",
                "--app",
                "--columns=application,name,version,origin",
            ],
        ),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn update_flatpak(username: &str, app_id: Option<&str>) -> ActionSpec {
    let mechanism = match app_id {
        Some(id) => flatpak_as(username, &["update", "--user", "-y", id]),
        None => flatpak_as(username, &["update", "--user", "-y"]),
    };
    ActionSpec {
        action_name: "UpdateFlatpak",
        mechanism,
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn get_flatpak_app_info(username: &str, app_id: &str) -> ActionSpec {
    ActionSpec {
        action_name: "GetFlatpakAppInfo",
        mechanism: flatpak_as(username, &["info", "--user", app_id]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}
