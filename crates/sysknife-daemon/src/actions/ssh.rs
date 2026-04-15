use super::{command_mechanism, ActionMechanism, ActionSpec};
use sysknife_types::RiskLevel;

pub fn specs() -> Vec<ActionSpec> {
    vec![
        get_authorized_keys("alice"),
        add_authorized_key("alice", "ssh-ed25519 AAAA..."),
        remove_authorized_key("alice", "ssh-ed25519 AAAA..."),
    ]
}

pub fn get_authorized_keys(username: &str) -> ActionSpec {
    ActionSpec {
        action_name: "GetAuthorizedKeys",
        mechanism: command_mechanism("cat", [&format!("/home/{username}/.ssh/authorized_keys")]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn add_authorized_key(username: &str, public_key: &str) -> ActionSpec {
    let keys_path = format!("/home/{username}/.ssh/authorized_keys");
    // Use sh -c with grep to check idempotency: only append if not already present.
    let script = format!(
        "grep -Fxq '{key}' '{path}' 2>/dev/null || echo '{key}' >> '{path}'",
        key = public_key,
        path = keys_path,
    );

    ActionSpec {
        action_name: "AddAuthorizedKey",
        mechanism: ActionMechanism::Command {
            program: "sh",
            args: vec!["-c".to_string(), script],
        },
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn remove_authorized_key(username: &str, public_key: &str) -> ActionSpec {
    let keys_path = format!("/home/{username}/.ssh/authorized_keys");
    // Use sed to delete the exact matching line.
    let script = format!(
        "sed -i '\\|^{key}$|d' '{path}'",
        key = public_key,
        path = keys_path,
    );

    ActionSpec {
        action_name: "RemoveAuthorizedKey",
        mechanism: ActionMechanism::Command {
            program: "sh",
            args: vec!["-c".to_string(), script],
        },
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}
