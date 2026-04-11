use crate::actions::{
    containers, deployment, flatpak, identity, layering, network, package_repos, services, toolbox,
    users, ActionMechanism, ActionSpec,
};
use serde_json::Value;
use std::io;

#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("unknown action: {0}")]
    UnknownAction(String),

    #[error("missing required param: {0}")]
    MissingParam(&'static str),

    #[error("invalid param type for: {0}")]
    InvalidParam(&'static str),

    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Map an action name and JSON params to an [`ActionSpec`].
///
/// Returns [`ExecutorError::UnknownAction`] for unrecognised names and
/// [`ExecutorError::MissingParam`] when a required param is absent.
pub fn build_action_spec(action_name: &str, params: &Value) -> Result<ActionSpec, ExecutorError> {
    match action_name {
        // ── Deployment: no params ─────────────────────────────────────────
        "GetSystemState" => Ok(deployment::get_system_state()),
        "CollectDiagnostics" => Ok(deployment::collect_diagnostics()),
        "GetDeploymentHistory" => Ok(deployment::get_deployment_history()),
        "ListDeployments" => Ok(deployment::list_deployments()),
        "UpdateSystem" => Ok(deployment::update_system()),
        "CleanupDeployments" => Ok(deployment::cleanup_deployments()),
        "RebootSystem" => Ok(deployment::reboot_system()),
        "RollbackDeployment" => Ok(deployment::rollback_deployment()),
        "GetKernelArguments" => Ok(deployment::get_kernel_arguments()),

        // ── Deployment: parameterized ─────────────────────────────────────
        "PinDeployment" => Ok(deployment::pin_deployment(require_u32(params, "index")?)),
        "UnpinDeployment" => Ok(deployment::unpin_deployment(require_u32(params, "index")?)),
        "RebaseSystem" => Ok(deployment::rebase_system(require_str(
            params,
            "target_ref",
        )?)),
        "SetKernelArguments" => {
            let add = str_array_or_empty(params, "add")?;
            let remove = str_array_or_empty(params, "remove")?;
            let add_refs: Vec<&str> = add.iter().map(String::as_str).collect();
            let remove_refs: Vec<&str> = remove.iter().map(String::as_str).collect();
            Ok(deployment::set_kernel_arguments(&add_refs, &remove_refs))
        }

        // ── Flatpak ───────────────────────────────────────────────────────
        "ListFlatpakRemotes" => Ok(flatpak::list_flatpak_remotes()),
        "InstallFlatpak" => Ok(flatpak::install_flatpak(
            require_str(params, "app_id")?,
            require_str(params, "remote")?,
        )),
        "RemoveFlatpak" => Ok(flatpak::remove_flatpak(require_str(params, "app_id")?)),
        "SearchFlatpakApps" => Ok(flatpak::search_flatpak_apps(require_str(params, "term")?)),
        "AddFlatpakRemote" => Ok(flatpak::add_flatpak_remote(
            require_str(params, "remote")?,
            require_str(params, "url")?,
        )),
        "RemoveFlatpakRemote" => Ok(flatpak::remove_flatpak_remote(require_str(
            params, "remote",
        )?)),
        "GetFlatpakAppInfo" => Ok(flatpak::get_flatpak_app_info(require_str(
            params, "app_id",
        )?)),

        // ── Containers ────────────────────────────────────────────────────
        "ListContainers" => Ok(containers::list_containers()),
        "CreateContainer" => Ok(containers::create_container(
            require_str(params, "name")?,
            require_str(params, "image")?,
        )),
        "StartContainer" => Ok(containers::start_container(require_str(params, "name")?)),
        "StopContainer" => Ok(containers::stop_container(require_str(params, "name")?)),
        "RemoveContainer" => Ok(containers::remove_container(require_str(params, "name")?)),
        "GetContainerInfo" => Ok(containers::get_container_info(require_str(params, "name")?)),

        // ── Layering ──────────────────────────────────────────────────────
        "GetLayeredPackages" => Ok(layering::get_layered_packages()),
        "ResetLayeredPackageOverride" => Ok(layering::reset_layered_package_override()),
        "InstallPackages" => {
            let pkgs = str_array_or_empty(params, "packages")?;
            let refs: Vec<&str> = pkgs.iter().map(String::as_str).collect();
            Ok(layering::install_packages(&refs))
        }
        "RemovePackages" => {
            let pkgs = str_array_or_empty(params, "packages")?;
            let refs: Vec<&str> = pkgs.iter().map(String::as_str).collect();
            Ok(layering::remove_packages(&refs))
        }
        "AddLayeredPackage" => Ok(layering::add_layered_package(require_str(
            params, "package",
        )?)),
        "RemoveLayeredPackage" => Ok(layering::remove_layered_package(require_str(
            params, "package",
        )?)),
        "ReplaceLayeredPackage" => Ok(layering::replace_layered_package(
            require_str(params, "old")?,
            require_str(params, "new")?,
        )),

        // ── Package repositories ──────────────────────────────────────────
        "ListPackageRepositories" => Ok(package_repos::list_package_repositories()),
        "AddPackageRepository" => Ok(package_repos::add_package_repository(
            validated_repo_id(params)?,
            validated_no_newline(params, "repo_url")?,
        )),
        "RemovePackageRepository" => {
            Ok(package_repos::remove_package_repository(validated_repo_id(params)?))
        }
        "EnablePackageRepository" => {
            Ok(package_repos::enable_package_repository(validated_repo_id(params)?))
        }
        "DisablePackageRepository" => {
            Ok(package_repos::disable_package_repository(validated_repo_id(params)?))
        }

        // ── Services ─────────────────────────────────────────────────────
        "ListServices" => Ok(services::list_services()),
        "StartService" => Ok(services::start_service(require_str(params, "unit")?)),
        "StopService" => Ok(services::stop_service(require_str(params, "unit")?)),
        "RestartService" => Ok(services::restart_service(require_str(params, "unit")?)),
        "SetServiceEnabled" => Ok(services::set_service_enabled(
            require_str(params, "unit")?,
            require_bool(params, "enabled")?,
        )),
        "MaskService" => Ok(services::mask_service(require_str(params, "unit")?)),
        "UnmaskService" => Ok(services::unmask_service(require_str(params, "unit")?)),
        "GetServiceLogs" => Ok(services::get_service_logs(require_str(params, "unit")?)),

        // ── Toolbox ───────────────────────────────────────────────────────
        "ListToolboxes" => Ok(toolbox::list_toolboxes()),
        "CreateToolbox" => Ok(toolbox::create_toolbox(
            require_str(params, "name")?,
            params.get("release").and_then(|v| v.as_str()),
            params.get("image").and_then(|v| v.as_str()),
        )),
        "EnterToolbox" => Ok(toolbox::enter_toolbox(require_str(params, "name")?)),
        "RemoveToolbox" => Ok(toolbox::remove_toolbox(require_str(params, "name")?)),

        // ── Identity ─────────────────────────────────────────────────────
        "SetHostname" => Ok(identity::set_hostname(require_str(params, "hostname")?)),
        "SetTimezone" => Ok(identity::set_timezone(require_str(params, "timezone")?)),
        "SetLocale" => Ok(identity::set_locale(require_str(params, "locale")?)),
        "SetNtp" => Ok(identity::set_ntp(require_bool(params, "enabled")?)),

        // ── Network ───────────────────────────────────────────────────────
        "GetFirewallState" => Ok(network::get_firewall_state()),
        "ConfigureWifi" => Ok(network::configure_wifi(require_str(params, "ssid")?)),
        "SetDnsServers" => {
            let servers = str_array_or_empty(params, "servers")?;
            let refs: Vec<&str> = servers.iter().map(String::as_str).collect();
            Ok(network::set_dns_servers(
                require_str(params, "interface")?,
                &refs,
            ))
        }
        "ConfigureFirewall" => Ok(network::configure_firewall(
            require_str(params, "zone")?,
            require_str(params, "service")?,
            require_bool(params, "enabled")?,
        )),

        // ── Users ─────────────────────────────────────────────────────────
        "ListUsers" => Ok(users::list_users()),
        "ListGroups" => Ok(users::list_groups()),
        "CreateUser" => Ok(users::create_user(
            require_str(params, "username")?,
            params.get("shell").and_then(|v| v.as_str()),
            params.get("home").and_then(|v| v.as_str()),
        )),
        "DeleteUser" => Ok(users::delete_user(require_str(params, "username")?)),
        "AddUserToGroup" => Ok(users::add_user_to_group(
            require_str(params, "username")?,
            require_str(params, "group")?,
        )),
        "RemoveUserFromGroup" => Ok(users::remove_user_from_group(
            require_str(params, "username")?,
            require_str(params, "group")?,
        )),

        _ => Err(ExecutorError::UnknownAction(action_name.to_string())),
    }
}

/// Execute an [`ActionSpec`] and return the output.
///
/// For `Command` mechanisms, the process is spawned and its stdout/stderr
/// are captured. For file mechanisms, the operation is performed directly
/// on the filesystem and an empty stdout is returned.
pub async fn execute_spec(spec: &ActionSpec) -> Result<ExecutionOutput, ExecutorError> {
    match &spec.mechanism {
        ActionMechanism::Command { program, args } => {
            let output = tokio::process::Command::new(program)
                .args(args)
                .output()
                .await?;
            Ok(ExecutionOutput {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.status.code().unwrap_or(-1),
            })
        }
        ActionMechanism::FileScan { path } => {
            let mut entries = tokio::fs::read_dir(path).await?;
            let mut names = Vec::new();
            while let Some(entry) = entries.next_entry().await? {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
            names.sort();
            Ok(ExecutionOutput {
                stdout: names.join("\n"),
                stderr: String::new(),
                exit_code: 0,
            })
        }
        ActionMechanism::FileWrite { path, content } => {
            if let Some(parent) = std::path::Path::new(path).parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(path, content).await?;
            Ok(ExecutionOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
        ActionMechanism::FilePatch {
            path,
            search,
            replace,
        } => {
            let content = tokio::fs::read_to_string(path).await?;
            let patched = content.replacen(search.as_str(), replace.as_str(), 1);
            tokio::fs::write(path, patched).await?;
            Ok(ExecutionOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
        ActionMechanism::FileDelete { path } => {
            tokio::fs::remove_file(path).await?;
            Ok(ExecutionOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
    }
}

fn require_str<'a>(params: &'a Value, key: &'static str) -> Result<&'a str, ExecutorError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or(ExecutorError::MissingParam(key))
}

/// Validate a repo_id: must be non-empty and contain only ASCII letters,
/// digits, hyphens, and underscores. Rejects `/`, `.`, and whitespace to
/// prevent path traversal (e.g. `../cron.d/evil`) and shell injection.
fn validated_repo_id(params: &Value) -> Result<&str, ExecutorError> {
    let id = require_str(params, "repo_id")?;
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(id)
    } else {
        Err(ExecutorError::InvalidParam("repo_id"))
    }
}

/// Validate that a string contains no newlines. Used for repo_url to prevent
/// INI-section injection into `.repo` file content.
fn validated_no_newline<'a>(
    params: &'a Value,
    key: &'static str,
) -> Result<&'a str, ExecutorError> {
    let val = require_str(params, key)?;
    if val.contains('\n') || val.contains('\r') {
        Err(ExecutorError::InvalidParam(key))
    } else {
        Ok(val)
    }
}

fn require_bool(params: &Value, key: &'static str) -> Result<bool, ExecutorError> {
    params
        .get(key)
        .and_then(|v| v.as_bool())
        .ok_or(ExecutorError::MissingParam(key))
}

fn require_u32(params: &Value, key: &'static str) -> Result<u32, ExecutorError> {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .ok_or(ExecutorError::MissingParam(key))
}

/// Returns a vec of owned strings from a JSON array, or an empty vec if the
/// key is absent or null. Returns [`ExecutorError::InvalidParam`] if the key
/// is present but not an array of strings.
fn str_array_or_empty(params: &Value, key: &'static str) -> Result<Vec<String>, ExecutorError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .ok_or(ExecutorError::InvalidParam(key))
            })
            .collect(),
        _ => Err(ExecutorError::InvalidParam(key)),
    }
}

/// Return the rollback [`ActionSpec`] for `action_name`, or `None` if no
/// automatic rollback is defined.
///
/// Only the five rpm-ostree deployment actions support rollback — they all
/// revert via `rpm-ostree rollback`. All other actions either have no sensible
/// rollback or are low-risk enough that a rollback would be net-harmful.
///
/// `RollbackDeployment` itself is excluded to prevent infinite recursion.
pub fn rollback_spec_for(action_name: &str) -> Option<ActionSpec> {
    match action_name {
        "UpdateSystem" | "InstallPackages" | "RemovePackages" | "RebaseSystem"
        | "SetKernelArguments" => Some(ActionSpec {
            action_name: "RollbackDeployment",
            mechanism: ActionMechanism::Command {
                program: "rpm-ostree",
                args: vec!["rollback".to_string()],
            },
            risk_level: lacs_types::RiskLevel::High,
            reboot_required: true,
            rollback_available: false,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lacs_types::RiskLevel;
    use serde_json::json;
    use tempfile::tempdir;

    // ── build_action_spec ─────────────────────────────────────────────────

    #[test]
    fn build_spec_no_params_for_get_system_state() {
        let spec = build_action_spec("GetSystemState", &json!({})).unwrap();
        assert_eq!(spec.action_name, "GetSystemState");
        assert_eq!(spec.risk_level, RiskLevel::Low);
    }

    #[test]
    fn build_spec_unknown_action_returns_error() {
        let err = build_action_spec("NonExistent", &json!({})).unwrap_err();
        assert!(
            matches!(&err, ExecutorError::UnknownAction(n) if n == "NonExistent"),
            "expected UnknownAction, got: {err}"
        );
    }

    #[test]
    fn build_spec_missing_param_for_install_flatpak() {
        let err = build_action_spec("InstallFlatpak", &json!({})).unwrap_err();
        assert!(
            matches!(err, ExecutorError::MissingParam("app_id")),
            "expected MissingParam(app_id), got: {err}"
        );
    }

    #[test]
    fn build_spec_install_flatpak_injects_app_and_remote() {
        let spec = build_action_spec(
            "InstallFlatpak",
            &json!({ "app_id": "org.mozilla.firefox", "remote": "flathub" }),
        )
        .unwrap();
        assert_eq!(spec.action_name, "InstallFlatpak");
        assert_eq!(
            spec.mechanism,
            ActionMechanism::Command {
                program: "flatpak",
                args: vec![
                    "install".to_string(),
                    "-y".to_string(),
                    "flathub".to_string(),
                    "org.mozilla.firefox".to_string(),
                ],
            }
        );
    }

    #[test]
    fn build_spec_pin_deployment_injects_index() {
        let spec = build_action_spec("PinDeployment", &json!({ "index": 1 })).unwrap();
        assert_eq!(
            spec.mechanism,
            ActionMechanism::Command {
                program: "ostree",
                args: vec!["admin".to_string(), "pin".to_string(), "1".to_string()],
            }
        );
    }

    #[test]
    fn build_spec_unpin_deployment_includes_unpin_flag() {
        let spec = build_action_spec("UnpinDeployment", &json!({ "index": 2 })).unwrap();
        assert_eq!(
            spec.mechanism,
            ActionMechanism::Command {
                program: "ostree",
                args: vec![
                    "admin".to_string(),
                    "pin".to_string(),
                    "--unpin".to_string(),
                    "2".to_string(),
                ],
            }
        );
    }

    #[test]
    fn build_spec_rebase_system_injects_target_ref() {
        let spec = build_action_spec(
            "RebaseSystem",
            &json!({ "target_ref": "fedora/41/x86_64/silverblue" }),
        )
        .unwrap();
        assert_eq!(
            spec.mechanism,
            ActionMechanism::Command {
                program: "rpm-ostree",
                args: vec![
                    "rebase".to_string(),
                    "fedora/41/x86_64/silverblue".to_string(),
                ],
            }
        );
    }

    #[test]
    fn build_spec_set_kernel_arguments_appends_and_deletes() {
        let spec = build_action_spec(
            "SetKernelArguments",
            &json!({ "add": ["mitigations=off"], "remove": ["quiet"] }),
        )
        .unwrap();
        assert_eq!(
            spec.mechanism,
            ActionMechanism::Command {
                program: "rpm-ostree",
                args: vec![
                    "kargs".to_string(),
                    "--append=mitigations=off".to_string(),
                    "--delete=quiet".to_string(),
                ],
            }
        );
    }

    #[test]
    fn build_spec_set_kernel_arguments_with_empty_arrays() {
        let spec =
            build_action_spec("SetKernelArguments", &json!({ "add": [], "remove": [] })).unwrap();
        assert_eq!(
            spec.mechanism,
            ActionMechanism::Command {
                program: "rpm-ostree",
                args: vec!["kargs".to_string()],
            }
        );
    }

    #[test]
    fn build_spec_set_kernel_arguments_defaults_when_keys_absent() {
        let spec = build_action_spec("SetKernelArguments", &json!({})).unwrap();
        assert_eq!(
            spec.mechanism,
            ActionMechanism::Command {
                program: "rpm-ostree",
                args: vec!["kargs".to_string()],
            }
        );
    }

    // ── execute_spec ──────────────────────────────────────────────────────

    #[test]
    fn build_spec_add_package_repository_rejects_path_traversal() {
        let err = build_action_spec(
            "AddPackageRepository",
            &json!({ "repo_id": "../cron.d/evil", "repo_url": "https://evil.example/repo" }),
        )
        .unwrap_err();
        assert!(
            matches!(err, ExecutorError::InvalidParam("repo_id")),
            "expected InvalidParam(repo_id), got: {err}"
        );
    }

    #[test]
    fn build_spec_add_package_repository_rejects_newline_in_url() {
        let err = build_action_spec(
            "AddPackageRepository",
            &json!({ "repo_id": "myrepo", "repo_url": "https://ok.example/\nbaseurl=evil" }),
        )
        .unwrap_err();
        assert!(
            matches!(err, ExecutorError::InvalidParam("repo_url")),
            "expected InvalidParam(repo_url), got: {err}"
        );
    }

    #[test]
    fn build_spec_add_package_repository_accepts_valid_repo_id() {
        let spec = build_action_spec(
            "AddPackageRepository",
            &json!({ "repo_id": "my-repo_123", "repo_url": "https://ok.example/repo" }),
        )
        .unwrap();
        assert_eq!(spec.action_name, "AddPackageRepository");
    }

    #[test]
    fn build_spec_remove_package_repository_rejects_path_traversal() {
        let err = build_action_spec(
            "RemovePackageRepository",
            &json!({ "repo_id": "../../etc/passwd" }),
        )
        .unwrap_err();
        assert!(
            matches!(err, ExecutorError::InvalidParam("repo_id")),
            "expected InvalidParam(repo_id), got: {err}"
        );
    }

    #[tokio::test]
    async fn execute_spec_command_captures_stdout() {
        let spec = ActionSpec {
            action_name: "GetSystemState",
            mechanism: ActionMechanism::Command {
                program: "echo",
                args: vec!["hello".to_string()],
            },
            risk_level: RiskLevel::Low,
            reboot_required: false,
            rollback_available: false,
        };
        let out = execute_spec(&spec).await.unwrap();
        assert_eq!(out.stdout.trim(), "hello");
        assert_eq!(out.exit_code, 0);
    }

    #[tokio::test]
    async fn execute_spec_file_write_creates_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.conf").to_string_lossy().into_owned();
        let spec = ActionSpec {
            action_name: "AddPackageRepository",
            mechanism: ActionMechanism::FileWrite {
                path: path.clone(),
                content: "[repo]\nbaseurl=https://example.test\n".to_string(),
            },
            risk_level: RiskLevel::Medium,
            reboot_required: false,
            rollback_available: true,
        };
        let out = execute_spec(&spec).await.unwrap();
        assert_eq!(out.exit_code, 0);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[repo]\nbaseurl=https://example.test\n"
        );
    }

    #[tokio::test]
    async fn execute_spec_file_patch_replaces_first_occurrence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("repo.conf").to_string_lossy().into_owned();
        std::fs::write(&path, "[myrepo]\nenabled=0\n").unwrap();
        let spec = ActionSpec {
            action_name: "EnablePackageRepository",
            mechanism: ActionMechanism::FilePatch {
                path: path.clone(),
                search: "enabled=0".to_string(),
                replace: "enabled=1".to_string(),
            },
            risk_level: RiskLevel::Medium,
            reboot_required: false,
            rollback_available: true,
        };
        execute_spec(&spec).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[myrepo]\nenabled=1\n"
        );
    }

    #[tokio::test]
    async fn execute_spec_file_delete_removes_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("repo.conf").to_string_lossy().into_owned();
        std::fs::write(&path, "[myrepo]\n").unwrap();
        let spec = ActionSpec {
            action_name: "RemovePackageRepository",
            mechanism: ActionMechanism::FileDelete { path: path.clone() },
            risk_level: RiskLevel::Medium,
            reboot_required: false,
            rollback_available: true,
        };
        execute_spec(&spec).await.unwrap();
        assert!(!std::path::Path::new(&path).exists());
    }

    #[tokio::test]
    async fn execute_spec_file_scan_lists_directory_entries() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.repo"), "[a]\n").unwrap();
        std::fs::write(dir.path().join("b.repo"), "[b]\n").unwrap();
        let spec = ActionSpec {
            action_name: "ListPackageRepositories",
            mechanism: ActionMechanism::FileScan {
                path: dir.path().to_string_lossy().into_owned(),
            },
            risk_level: RiskLevel::Low,
            reboot_required: false,
            rollback_available: false,
        };
        let out = execute_spec(&spec).await.unwrap();
        assert!(
            out.stdout.contains("a.repo"),
            "expected a.repo in: {}",
            out.stdout
        );
        assert!(
            out.stdout.contains("b.repo"),
            "expected b.repo in: {}",
            out.stdout
        );
        assert_eq!(out.exit_code, 0);
    }

    // ── rollback_spec_for ─────────────────────────────────────────────────────

    #[test]
    fn rollback_spec_for_update_system_is_rpm_ostree_rollback() {
        let spec = rollback_spec_for("UpdateSystem").unwrap();
        assert_eq!(spec.action_name, "RollbackDeployment");
        assert!(
            matches!(
                &spec.mechanism,
                ActionMechanism::Command { program: "rpm-ostree", args }
                if args == &["rollback".to_string()]
            ),
            "expected rpm-ostree rollback, got: {:?}",
            spec.mechanism
        );
        assert!(!spec.rollback_available, "rollback spec must not recurse");
    }

    #[test]
    fn rollback_spec_for_install_packages_is_rpm_ostree_rollback() {
        assert!(rollback_spec_for("InstallPackages").is_some());
    }

    #[test]
    fn rollback_spec_for_remove_packages_is_rpm_ostree_rollback() {
        assert!(rollback_spec_for("RemovePackages").is_some());
    }

    #[test]
    fn rollback_spec_for_rebase_system_is_rpm_ostree_rollback() {
        assert!(rollback_spec_for("RebaseSystem").is_some());
    }

    #[test]
    fn rollback_spec_for_set_kernel_arguments_is_rpm_ostree_rollback() {
        assert!(rollback_spec_for("SetKernelArguments").is_some());
    }

    #[test]
    fn rollback_spec_for_read_only_action_returns_none() {
        assert!(rollback_spec_for("GetSystemState").is_none());
        assert!(rollback_spec_for("ListUsers").is_none());
        assert!(rollback_spec_for("GetFirewallState").is_none());
    }

    #[test]
    fn rollback_spec_for_non_rollbackable_actions_return_none() {
        assert!(rollback_spec_for("AddUserToGroup").is_none());
        assert!(rollback_spec_for("DeleteUser").is_none());
        assert!(rollback_spec_for("CleanupDeployments").is_none());
        // No infinite recursion — RollbackDeployment has no rollback of its own
        assert!(rollback_spec_for("RollbackDeployment").is_none());
    }
}
