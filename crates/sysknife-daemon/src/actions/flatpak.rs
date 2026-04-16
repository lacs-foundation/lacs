use super::{command_mechanism, ActionSpec, ActionMechanism};
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

/// Run a Flatpak command as the target user via `sudo runuser -l`.
///
/// Flatpak user installations live under `~/.local/share/flatpak/` and are
/// accessed through the user's D-Bus session. The daemon runs as `sysknife`
/// (a system user) with no user installation; `runuser -l` switches to the
/// correct user environment so `--user` operations reach the right store.
fn flatpak_as(username: &str, flatpak_cmd: &str) -> ActionMechanism {
    ActionMechanism::Command {
        program: "sudo",
        args: vec![
            "runuser".to_string(),
            "-l".to_string(),
            username.to_string(),
            "-c".to_string(),
            flatpak_cmd.to_string(),
        ],
    }
}

pub fn install_flatpak(username: &str, app_id: &str, remote: &str) -> ActionSpec {
    let cmd = format!("flatpak install --user -y '{}' '{}'", remote, app_id);
    ActionSpec {
        action_name: "InstallFlatpak",
        mechanism: flatpak_as(username, &cmd),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn remove_flatpak(username: &str, app_id: &str) -> ActionSpec {
    let cmd = format!("flatpak uninstall --user -y '{}'", app_id);
    ActionSpec {
        action_name: "RemoveFlatpak",
        mechanism: flatpak_as(username, &cmd),
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
        mechanism: flatpak_as(
            username,
            "flatpak remotes --user --columns=name,url",
        ),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn add_flatpak_remote(username: &str, remote: &str, url: &str) -> ActionSpec {
    let cmd = format!(
        "flatpak remote-add --user --if-not-exists '{}' '{}'",
        remote, url
    );
    ActionSpec {
        action_name: "AddFlatpakRemote",
        mechanism: flatpak_as(username, &cmd),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn remove_flatpak_remote(username: &str, remote: &str) -> ActionSpec {
    let cmd = format!("flatpak remote-delete --user '{}'", remote);
    ActionSpec {
        action_name: "RemoveFlatpakRemote",
        mechanism: flatpak_as(username, &cmd),
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
            "flatpak list --user --app --columns=application,name,version,origin",
        ),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn update_flatpak(username: &str, app_id: Option<&str>) -> ActionSpec {
    let cmd = match app_id {
        Some(id) => format!("flatpak update --user -y '{}'", id),
        None => "flatpak update --user -y".to_string(),
    };
    ActionSpec {
        action_name: "UpdateFlatpak",
        mechanism: flatpak_as(username, &cmd),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn get_flatpak_app_info(username: &str, app_id: &str) -> ActionSpec {
    let cmd = format!("flatpak info --user '{}'", app_id);
    ActionSpec {
        action_name: "GetFlatpakAppInfo",
        mechanism: flatpak_as(username, &cmd),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}
