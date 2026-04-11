use serde::{Deserialize, Serialize};
use std::io;

/// Snapshot of live system state collected by the daemon.
///
/// Field names mirror `lacs_brain::CuratedState` so the shell can deserialize
/// the JSON representation without depending on this crate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectedState {
    pub host_name: String,
    pub deployment: String,
    pub services: Vec<String>,
    pub flatpaks: Vec<String>,
    pub toolboxes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CollectorError {
    #[error("command failed for {command}: {reason}")]
    CommandFailed {
        command: &'static str,
        reason: String,
    },
}

/// Abstraction over command execution, making state collection testable
/// without requiring system tools to be installed.
pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, io::Error>;
}

/// Production implementation that delegates to `std::process::Command`.
pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, io::Error> {
        let output = std::process::Command::new(program).args(args).output()?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Collect a system state snapshot using the provided runner.
///
/// Only `host_name` is required. All other fields (`deployment`, `services`,
/// `flatpaks`, `toolboxes`) are best-effort and default to empty on failure so
/// the daemon works on non-Silverblue systems and in CI. The brain's safety
/// fence will reject irrelevant actions based on the curated state it receives.
pub fn collect_state(runner: &dyn CommandRunner) -> Result<CollectedState, CollectorError> {
    let host_name = runner
        .run("hostname", &[])
        .map(|s| s.trim().to_string())
        .map_err(|e| CollectorError::CommandFailed {
            command: "hostname",
            reason: e.to_string(),
        })?;

    let deployment = runner
        .run("rpm-ostree", &["status", "--booted", "--json"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let services = runner
        .run(
            "systemctl",
            &[
                "list-units",
                "--type=service",
                "--state=running",
                "--no-legend",
                "--no-pager",
                "--plain",
            ],
        )
        .map(|s| parse_service_lines(&s))
        .unwrap_or_default();

    let flatpaks = runner
        .run("flatpak", &["list", "--columns=application"])
        .map(|s| parse_lines(&s))
        .unwrap_or_default();

    let toolboxes = runner
        .run("toolbox", &["list", "--containers"])
        .map(|s| parse_lines(&s))
        .unwrap_or_default();

    Ok(CollectedState {
        host_name,
        deployment,
        services,
        flatpaks,
        toolboxes,
    })
}

/// Extract the first whitespace-delimited field from each non-empty line.
/// Suitable for systemctl's columnar output where the first field is the unit name.
fn parse_service_lines(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(String::from)
        .collect()
}

/// Return non-empty trimmed lines from command output.
fn parse_lines(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockRunner {
        responses: HashMap<String, String>,
    }

    impl MockRunner {
        fn new(entries: &[(&str, &[&str], &str)]) -> Self {
            let responses = entries
                .iter()
                .map(|(program, args, output)| {
                    let key = std::iter::once(*program)
                        .chain(args.iter().copied())
                        .collect::<Vec<_>>()
                        .join(" ");
                    (key, output.to_string())
                })
                .collect();
            Self { responses }
        }
    }

    impl CommandRunner for MockRunner {
        fn run(&self, program: &str, args: &[&str]) -> Result<String, io::Error> {
            let key = std::iter::once(program)
                .chain(args.iter().copied())
                .collect::<Vec<_>>()
                .join(" ");
            Ok(self.responses.get(&key).cloned().unwrap_or_default())
        }
    }

    #[test]
    fn collect_state_parses_hostname_and_deployment() {
        let runner = MockRunner::new(&[
            ("hostname", &[], "silverblue-lab\n"),
            (
                "rpm-ostree",
                &["status", "--booted", "--json"],
                r#"{"deployments":[]}"#,
            ),
            (
                "systemctl",
                &[
                    "list-units",
                    "--type=service",
                    "--state=running",
                    "--no-legend",
                    "--no-pager",
                    "--plain",
                ],
                "sshd.service  loaded  active  running  OpenSSH server\n",
            ),
            (
                "flatpak",
                &["list", "--columns=application"],
                "org.gnome.Gedit\n",
            ),
            ("toolbox", &["list", "--containers"], "lacs-dev\n"),
        ]);

        let state = collect_state(&runner).unwrap();

        assert_eq!(state.host_name, "silverblue-lab");
        assert_eq!(state.deployment, r#"{"deployments":[]}"#);
        assert_eq!(state.services, vec!["sshd.service"]);
        assert_eq!(state.flatpaks, vec!["org.gnome.Gedit"]);
        assert_eq!(state.toolboxes, vec!["lacs-dev"]);
    }

    #[test]
    fn collect_state_trims_hostname_whitespace() {
        let runner = MockRunner::new(&[
            ("hostname", &[], "  my-host  \n"),
            ("rpm-ostree", &["status", "--booted", "--json"], "{}"),
        ]);

        let state = collect_state(&runner).unwrap();
        assert_eq!(state.host_name, "my-host");
    }

    #[test]
    fn collect_state_defaults_to_empty_lists_on_optional_command_failure() {
        struct PartialRunner;
        impl CommandRunner for PartialRunner {
            fn run(&self, program: &str, _args: &[&str]) -> Result<String, io::Error> {
                match program {
                    "hostname" => Ok("host\n".to_string()),
                    "rpm-ostree" => Ok("{}".to_string()),
                    _ => Err(io::Error::new(io::ErrorKind::NotFound, "not found")),
                }
            }
        }

        let state = collect_state(&PartialRunner).unwrap();
        assert!(state.services.is_empty());
        assert!(state.flatpaks.is_empty());
        assert!(state.toolboxes.is_empty());
    }

    #[test]
    fn collect_state_returns_error_when_hostname_fails() {
        struct FailingRunner;
        impl CommandRunner for FailingRunner {
            fn run(&self, _program: &str, _args: &[&str]) -> Result<String, io::Error> {
                Err(io::Error::new(io::ErrorKind::NotFound, "not found"))
            }
        }

        let result = collect_state(&FailingRunner);
        assert!(
            matches!(
                result,
                Err(CollectorError::CommandFailed {
                    command: "hostname",
                    ..
                })
            ),
            "expected CommandFailed(hostname), got: {result:?}"
        );
    }

    #[test]
    fn collect_state_returns_empty_deployment_when_rpm_ostree_missing() {
        // rpm-ostree is not available on all systems (e.g. Fedora Workstation,
        // Ubuntu, CI). A missing binary must produce an empty deployment string,
        // not an error, so the daemon stays functional on non-Silverblue hosts.
        struct NoRpmOstreeRunner;
        impl CommandRunner for NoRpmOstreeRunner {
            fn run(&self, program: &str, _args: &[&str]) -> Result<String, io::Error> {
                match program {
                    "hostname" => Ok("non-silverblue-host\n".to_string()),
                    _ => Err(io::Error::new(io::ErrorKind::NotFound, "not found")),
                }
            }
        }

        let state = collect_state(&NoRpmOstreeRunner).unwrap();
        assert_eq!(state.host_name, "non-silverblue-host");
        assert_eq!(state.deployment, "", "deployment must be empty, not an error");
        assert!(state.services.is_empty());
        assert!(state.flatpaks.is_empty());
        assert!(state.toolboxes.is_empty());
    }

    #[test]
    fn collect_state_parses_multiple_services() {
        let runner = MockRunner::new(&[
            ("hostname", &[], "host\n"),
            ("rpm-ostree", &["status", "--booted", "--json"], "{}"),
            (
                "systemctl",
                &[
                    "list-units",
                    "--type=service",
                    "--state=running",
                    "--no-legend",
                    "--no-pager",
                    "--plain",
                ],
                "sshd.service  loaded  active  running  SSH\nNetworkManager.service  loaded  active  running  NM\n",
            ),
        ]);

        let state = collect_state(&runner).unwrap();
        assert_eq!(
            state.services,
            vec!["sshd.service", "NetworkManager.service"]
        );
    }

    #[test]
    fn collected_state_round_trips_through_json() {
        let state = CollectedState {
            host_name: "lab".to_string(),
            deployment: "{}".to_string(),
            services: vec!["sshd.service".to_string()],
            flatpaks: vec!["org.mozilla.firefox".to_string()],
            toolboxes: vec![],
        };

        let json = serde_json::to_string(&state).unwrap();
        let restored: CollectedState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, restored);
    }
}
