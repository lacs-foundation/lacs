use super::{command_mechanism, ActionSpec};
use lacs_types::RiskLevel;

pub fn specs() -> Vec<ActionSpec> {
    vec![
        list_toolboxes(),
        create_toolbox("lacs-dev", Some("41"), None),
        enter_toolbox("lacs-dev"),
        remove_toolbox("lacs-dev"),
    ]
}

pub fn list_toolboxes() -> ActionSpec {
    ActionSpec {
        action_name: "ListToolboxes",
        mechanism: command_mechanism("toolbox", ["list", "--containers"]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn create_toolbox(name: &str, release: Option<&str>, image: Option<&str>) -> ActionSpec {
    let mut args = vec![
        "create".to_string(),
        "--container".to_string(),
        name.to_string(),
    ];
    if let Some(release) = release {
        args.push("--release".to_string());
        args.push(release.to_string());
    }
    if let Some(image) = image {
        args.push("--image".to_string());
        args.push(image.to_string());
    }

    ActionSpec {
        action_name: "CreateToolbox",
        mechanism: super::ActionMechanism::Command {
            program: "toolbox",
            args,
        },
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn enter_toolbox(name: &str) -> ActionSpec {
    ActionSpec {
        action_name: "EnterToolbox",
        mechanism: command_mechanism("toolbox", ["enter", name]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn remove_toolbox(name: &str) -> ActionSpec {
    ActionSpec {
        action_name: "RemoveToolbox",
        mechanism: command_mechanism("toolbox", ["rm", name]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}
