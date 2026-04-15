use lacs_daemon::actions::identity;
use lacs_daemon::actions::network;
use lacs_daemon::actions::services;
use lacs_daemon::actions::users;
use lacs_daemon::actions::ActionMechanism;
use lacs_types::RiskLevel;

#[test]
fn services_family_covers_list_control_and_logs() {
    let names = services::specs()
        .into_iter()
        .map(|spec| spec.action_name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "ListServices",
            "StartService",
            "StopService",
            "RestartService",
            "SetServiceEnabled",
            "MaskService",
            "UnmaskService",
            "GetServiceLogs",
            "GetServiceStatus",
            "ReloadService",
            "ListTimers",
            "ReloadDaemon",
        ]
    );
}

#[test]
fn reload_service_uses_reload_not_restart() {
    // Regression guard: reload sends ExecReload= signal; restart stops+starts.
    // These have different availability/disruption profiles.
    let spec = services::reload_service("nginx.service");

    assert_eq!(spec.action_name, "ReloadService");
    assert_eq!(spec.risk_level, RiskLevel::Medium);
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "systemctl",
            args: vec!["reload".to_string(), "nginx.service".to_string()],
        }
    );
    // Explicitly guard against the wrong verb.
    if let ActionMechanism::Command { args, .. } = &spec.mechanism {
        assert!(
            !args.contains(&"restart".to_string()),
            "reload_service must use 'reload', not 'restart'"
        );
    }
}

#[test]
fn reload_daemon_uses_daemon_reload_subcommand_with_no_unit_arg() {
    let spec = services::reload_daemon();

    assert_eq!(spec.action_name, "ReloadDaemon");
    assert_eq!(spec.risk_level, RiskLevel::Medium);
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "systemctl",
            args: vec!["daemon-reload".to_string()],
        }
    );
    // daemon-reload takes no unit argument — verify exactly one arg.
    if let ActionMechanism::Command { args, .. } = &spec.mechanism {
        assert_eq!(args.len(), 1, "daemon-reload must have no unit argument");
    }
}

#[test]
fn get_service_status_uses_status_with_no_pager() {
    // --no-pager is required to prevent systemctl from blocking waiting for
    // terminal input when output exceeds the screen height (breaks CI / daemon).
    let spec = services::get_service_status("nginx.service");

    assert_eq!(spec.action_name, "GetServiceStatus");
    assert_eq!(spec.risk_level, RiskLevel::Low);
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "systemctl",
            args: vec![
                "status".to_string(),
                "nginx.service".to_string(),
                "--no-pager".to_string(),
            ],
        }
    );
    if let ActionMechanism::Command { args, .. } = &spec.mechanism {
        assert!(
            args.contains(&"--no-pager".to_string()),
            "--no-pager is required to prevent blocking"
        );
    }
}

#[test]
fn list_timers_includes_all_and_no_pager_flags() {
    // --all includes inactive timers (not just running ones).
    // --no-pager prevents blocking when output is long.
    let spec = services::list_timers();

    assert_eq!(spec.action_name, "ListTimers");
    assert_eq!(spec.risk_level, RiskLevel::Low);
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "systemctl",
            args: vec![
                "list-timers".to_string(),
                "--all".to_string(),
                "--no-pager".to_string(),
            ],
        }
    );
    if let ActionMechanism::Command { args, .. } = &spec.mechanism {
        assert!(
            args.contains(&"--all".to_string()),
            "--all is required to show inactive timers"
        );
    }
}

#[test]
fn get_firewall_state_uses_list_all_not_state() {
    // Bug fix regression: --state only returns "running"/"not running" with no
    // config detail. --list-all shows the active zone, services, ports, and rules.
    let spec = network::get_firewall_state();

    assert_eq!(spec.action_name, "GetFirewallState");
    assert_eq!(spec.risk_level, RiskLevel::Low);
    if let ActionMechanism::Command { args, .. } = &spec.mechanism {
        assert!(
            args.contains(&"--list-all".to_string()),
            "--list-all is required for full firewall config inspection"
        );
        assert!(
            !args.contains(&"--state".to_string()),
            "--state only returns running/not-running — too coarse for config inspection"
        );
    }
}

#[test]
fn restart_service_uses_systemctl_without_shell() {
    let spec = services::restart_service("NetworkManager.service");

    assert_eq!(spec.action_name, "RestartService");
    assert_eq!(spec.risk_level, RiskLevel::Medium);
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "systemctl",
            args: vec!["restart".to_string(), "NetworkManager.service".to_string()],
        }
    );
}

#[test]
fn service_logs_are_bounded() {
    let spec = services::get_service_logs("NetworkManager.service");

    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "journalctl",
            args: vec![
                "-u".to_string(),
                "NetworkManager.service".to_string(),
                "-n".to_string(),
                "200".to_string(),
                "--no-pager".to_string(),
            ],
        }
    );
}

#[test]
fn service_enable_and_disable_use_matching_systemctl_commands() {
    let enabled = services::set_service_enabled("sshd.service", true);
    let disabled = services::set_service_enabled("sshd.service", false);

    assert_eq!(
        enabled.mechanism,
        ActionMechanism::Command {
            program: "systemctl",
            args: vec!["enable".to_string(), "sshd.service".to_string()],
        }
    );
    assert_eq!(
        disabled.mechanism,
        ActionMechanism::Command {
            program: "systemctl",
            args: vec!["disable".to_string(), "sshd.service".to_string()],
        }
    );
}

#[test]
fn network_family_covers_wifi_dns_and_firewall() {
    let names = network::specs()
        .into_iter()
        .map(|spec| spec.action_name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "ConfigureWifi",
            "SetDnsServers",
            "ConfigureFirewall",
            "GetFirewallState",
            "GetNetworkStatus",
        ]
    );
}

#[test]
fn configure_wifi_uses_nmcli_connect_without_shell() {
    let spec = network::configure_wifi("CafeHotspot");

    assert_eq!(spec.action_name, "ConfigureWifi");
    assert_eq!(spec.risk_level, RiskLevel::Medium);
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "nmcli",
            args: vec![
                "device".to_string(),
                "wifi".to_string(),
                "connect".to_string(),
                "CafeHotspot".to_string(),
                "--ask".to_string(),
            ],
        }
    );
}

#[test]
fn set_dns_servers_uses_resolvectl() {
    let spec = network::set_dns_servers("wlp1s0", &["1.1.1.1", "8.8.8.8"]);

    assert_eq!(spec.action_name, "SetDnsServers");
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "resolvectl",
            args: vec![
                "dns".to_string(),
                "wlp1s0".to_string(),
                "1.1.1.1".to_string(),
                "8.8.8.8".to_string(),
            ],
        }
    );
}

#[test]
fn configure_firewall_uses_firewall_cmd_without_shell() {
    let spec = network::configure_firewall("public", "ssh", true);

    assert_eq!(spec.action_name, "ConfigureFirewall");
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "firewall-cmd",
            args: vec![
                "--zone".to_string(),
                "public".to_string(),
                "--add-service".to_string(),
                "ssh".to_string(),
            ],
        }
    );
}

#[test]
fn configure_firewall_disable_uses_firewall_cmd_without_shell() {
    let spec = network::configure_firewall("public", "ssh", false);

    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "firewall-cmd",
            args: vec![
                "--zone".to_string(),
                "public".to_string(),
                "--remove-service".to_string(),
                "ssh".to_string(),
            ],
        }
    );
}

#[test]
fn identity_family_covers_hostname_timezone_locale_and_ntp() {
    let names = identity::specs()
        .into_iter()
        .map(|spec| spec.action_name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec!["SetHostname", "SetTimezone", "SetLocale", "SetNtp"]
    );
}

#[test]
fn identity_changes_use_systemd_tools() {
    let hostname = identity::set_hostname("lacs-lab");
    let timezone = identity::set_timezone("America/Chicago");
    let locale = identity::set_locale("en_US.UTF-8");
    let ntp = identity::set_ntp(true);

    assert_eq!(
        hostname.mechanism,
        ActionMechanism::Command {
            program: "hostnamectl",
            args: vec!["hostname".to_string(), "lacs-lab".to_string()],
        }
    );
    assert_eq!(
        timezone.mechanism,
        ActionMechanism::Command {
            program: "timedatectl",
            args: vec!["set-timezone".to_string(), "America/Chicago".to_string()],
        }
    );
    assert_eq!(
        locale.mechanism,
        ActionMechanism::Command {
            program: "localectl",
            args: vec!["set-locale".to_string(), "en_US.UTF-8".to_string()],
        }
    );
    assert_eq!(
        ntp.mechanism,
        ActionMechanism::Command {
            program: "timedatectl",
            args: vec!["set-ntp".to_string(), "true".to_string()],
        }
    );
}

#[test]
fn users_family_covers_listing_and_account_management() {
    let names = users::specs()
        .into_iter()
        .map(|spec| spec.action_name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "ListUsers",
            "ListGroups",
            "CreateUser",
            "DeleteUser",
            "AddUserToGroup",
            "RemoveUserFromGroup",
        ]
    );
}

#[test]
fn user_creation_and_group_changes_use_sudo_prefixed_shadow_tools() {
    let create = users::create_user("alice", Some("/bin/bash"), Some("/home/alice"));
    let delete = users::delete_user("alice");
    let add_group = users::add_user_to_group("alice", "wheel");
    let remove_group = users::remove_user_from_group("alice", "wheel");

    assert_eq!(create.risk_level, RiskLevel::Medium);
    assert_eq!(
        create.mechanism,
        ActionMechanism::Command {
            program: "sudo",
            args: vec![
                "useradd".to_string(),
                "--create-home".to_string(),
                "--home-dir".to_string(),
                "/home/alice".to_string(),
                "--shell".to_string(),
                "/bin/bash".to_string(),
                "alice".to_string(),
            ],
        }
    );
    assert_eq!(
        delete.mechanism,
        ActionMechanism::Command {
            program: "sudo",
            args: vec!["userdel".to_string(), "alice".to_string()],
        }
    );
    assert_eq!(
        add_group.mechanism,
        ActionMechanism::Command {
            program: "sudo",
            args: vec![
                "usermod".to_string(),
                "--append".to_string(),
                "--groups".to_string(),
                "wheel".to_string(),
                "alice".to_string(),
            ],
        }
    );
    assert_eq!(
        remove_group.mechanism,
        ActionMechanism::Command {
            program: "sudo",
            args: vec![
                "gpasswd".to_string(),
                "--delete".to_string(),
                "alice".to_string(),
                "wheel".to_string()
            ],
        }
    );
}
