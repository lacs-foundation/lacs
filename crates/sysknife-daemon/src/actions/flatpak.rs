use super::{command_mechanism, ActionSpec};
use sysknife_types::RiskLevel;

pub fn specs() -> Vec<ActionSpec> {
    vec![
        install_flatpak("app-id", "flathub"),
        remove_flatpak("app-id"),
        search_flatpak_apps("search-term"),
        list_flatpak_remotes(),
        list_installed_flatpaks(),
        add_flatpak_remote("remote", "https://example.invalid"),
        remove_flatpak_remote("remote"),
        get_flatpak_app_info("app-id"),
        update_flatpak(Some("com.example.App")),
    ]
}

pub fn install_flatpak(app_id: &str, remote: &str) -> ActionSpec {
    ActionSpec {
        action_name: "InstallFlatpak",
        mechanism: command_mechanism("flatpak", ["install", "-y", remote, app_id]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn remove_flatpak(app_id: &str) -> ActionSpec {
    ActionSpec {
        action_name: "RemoveFlatpak",
        mechanism: command_mechanism("flatpak", ["uninstall", "-y", app_id]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn search_flatpak_apps(term: &str) -> ActionSpec {
    ActionSpec {
        action_name: "SearchFlatpakApps",
        mechanism: command_mechanism("flatpak", ["search", term]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn list_flatpak_remotes() -> ActionSpec {
    ActionSpec {
        action_name: "ListFlatpakRemotes",
        mechanism: command_mechanism("flatpak", ["remotes", "--columns=name,url"]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn add_flatpak_remote(remote: &str, url: &str) -> ActionSpec {
    ActionSpec {
        action_name: "AddFlatpakRemote",
        mechanism: command_mechanism("flatpak", ["remote-add", "--if-not-exists", remote, url]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn remove_flatpak_remote(remote: &str) -> ActionSpec {
    ActionSpec {
        action_name: "RemoveFlatpakRemote",
        mechanism: command_mechanism("flatpak", ["remote-delete", remote]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn list_installed_flatpaks() -> ActionSpec {
    ActionSpec {
        action_name: "ListInstalledFlatpaks",
        mechanism: command_mechanism(
            "flatpak",
            ["list", "--app", "--columns=application,name,version,origin"],
        ),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn update_flatpak(app_id: Option<&str>) -> ActionSpec {
    let mut args = vec!["update".to_string(), "-y".to_string()];
    if let Some(id) = app_id {
        args.push(id.to_string());
    }

    ActionSpec {
        action_name: "UpdateFlatpak",
        mechanism: super::ActionMechanism::Command {
            program: "flatpak",
            args,
        },
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn get_flatpak_app_info(app_id: &str) -> ActionSpec {
    ActionSpec {
        action_name: "GetFlatpakAppInfo",
        mechanism: command_mechanism("flatpak", ["info", app_id]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}
