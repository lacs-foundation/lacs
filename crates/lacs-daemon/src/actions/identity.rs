use super::{command_mechanism, ActionSpec};
use lacs_types::RiskLevel;

pub fn specs() -> Vec<ActionSpec> {
    vec![
        set_hostname("lacs-lab"),
        set_timezone("America/Mexico_City"),
        set_locale("en_US.UTF-8"),
        set_ntp(true),
    ]
}

pub fn set_hostname(hostname: &str) -> ActionSpec {
    ActionSpec {
        action_name: "SetHostname",
        mechanism: command_mechanism("hostnamectl", ["hostname", hostname]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn set_timezone(timezone: &str) -> ActionSpec {
    ActionSpec {
        action_name: "SetTimezone",
        mechanism: command_mechanism("timedatectl", ["set-timezone", timezone]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn set_locale(locale: &str) -> ActionSpec {
    ActionSpec {
        action_name: "SetLocale",
        mechanism: command_mechanism("localectl", ["set-locale", locale]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn set_ntp(enabled: bool) -> ActionSpec {
    ActionSpec {
        action_name: "SetNtp",
        mechanism: command_mechanism(
            "timedatectl",
            ["set-ntp", if enabled { "true" } else { "false" }],
        ),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}
