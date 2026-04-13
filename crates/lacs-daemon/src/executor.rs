use crate::actions::{
    containers, deployment, filesystem, flatpak, identity, layering, network, package_repos,
    processes, services, ssh, system_info, toolbox, users,
    validate::{
        validated_group, validated_hostname, validated_locale, validated_safe_arg,
        validated_timezone, validated_unit_name, validated_username,
    },
    ActionMechanism, ActionSpec,
};
use async_trait::async_trait;
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

/// Abstraction over action execution, making the execute + rollback path
/// testable without spawning real OS commands.
///
/// The production implementation (`RealActionExecutor`) delegates to
/// `tokio::process::Command`. Tests can inject a mock that controls exit
/// codes and output per program.
#[async_trait]
pub trait ActionExecutor: Send + Sync {
    /// Execute an [`ActionSpec`] and return its output.
    async fn execute(&self, spec: &ActionSpec) -> Result<ExecutionOutput, ExecutorError>;
}

/// Production executor that delegates to real OS processes and filesystem ops.
pub struct RealActionExecutor;

#[async_trait]
impl ActionExecutor for RealActionExecutor {
    async fn execute(&self, spec: &ActionSpec) -> Result<ExecutionOutput, ExecutorError> {
        execute_spec(spec).await
    }
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
        "RebaseSystem" => {
            let target_ref = require_str(params, "target_ref")?;
            let target_ref = validated_safe_arg(target_ref, "target_ref")?;
            Ok(deployment::rebase_system(&target_ref))
        }
        "SetKernelArguments" => {
            let add = str_array_or_empty(params, "add")?;
            let remove = str_array_or_empty(params, "remove")?;
            let add_refs: Vec<&str> = add.iter().map(String::as_str).collect();
            let remove_refs: Vec<&str> = remove.iter().map(String::as_str).collect();
            Ok(deployment::set_kernel_arguments(&add_refs, &remove_refs))
        }

        // ── Flatpak ───────────────────────────────────────────────────────
        "ListFlatpakRemotes" => Ok(flatpak::list_flatpak_remotes()),
        "InstallFlatpak" => {
            let app_id = validated_safe_arg(require_str(params, "app_id")?, "app_id")?;
            let remote = validated_safe_arg(require_str(params, "remote")?, "remote")?;
            Ok(flatpak::install_flatpak(&app_id, &remote))
        }
        "RemoveFlatpak" => {
            let app_id = validated_safe_arg(require_str(params, "app_id")?, "app_id")?;
            Ok(flatpak::remove_flatpak(&app_id))
        }
        "SearchFlatpakApps" => Ok(flatpak::search_flatpak_apps(require_str(params, "term")?)),
        "AddFlatpakRemote" => {
            let remote = validated_safe_arg(require_str(params, "remote")?, "remote")?;
            Ok(flatpak::add_flatpak_remote(
                &remote,
                require_str(params, "url")?,
            ))
        }
        "RemoveFlatpakRemote" => {
            let remote = validated_safe_arg(require_str(params, "remote")?, "remote")?;
            Ok(flatpak::remove_flatpak_remote(&remote))
        }
        "GetFlatpakAppInfo" => {
            let app_id = validated_safe_arg(require_str(params, "app_id")?, "app_id")?;
            Ok(flatpak::get_flatpak_app_info(&app_id))
        }

        // ── Containers ────────────────────────────────────────────────────
        "ListContainers" => Ok(containers::list_containers()),
        "CreateContainer" => {
            let name = validated_safe_arg(require_str(params, "name")?, "name")?;
            let image = validated_safe_arg(require_str(params, "image")?, "image")?;
            Ok(containers::create_container(&name, &image))
        }
        "StartContainer" => {
            let name = validated_safe_arg(require_str(params, "name")?, "name")?;
            Ok(containers::start_container(&name))
        }
        "StopContainer" => {
            let name = validated_safe_arg(require_str(params, "name")?, "name")?;
            Ok(containers::stop_container(&name))
        }
        "RemoveContainer" => {
            let name = validated_safe_arg(require_str(params, "name")?, "name")?;
            Ok(containers::remove_container(&name))
        }
        "GetContainerInfo" => {
            let name = validated_safe_arg(require_str(params, "name")?, "name")?;
            Ok(containers::get_container_info(&name))
        }

        // ── Layering ──────────────────────────────────────────────────────
        "GetLayeredPackages" => Ok(layering::get_layered_packages()),
        "ResetLayeredPackageOverride" => Ok(layering::reset_layered_package_override()),
        "InstallPackages" => {
            let pkgs = str_array_or_empty(params, "packages")?;
            let validated: Vec<String> = pkgs
                .iter()
                .map(|p| validated_safe_arg(p, "packages"))
                .collect::<Result<_, _>>()?;
            let refs: Vec<&str> = validated.iter().map(String::as_str).collect();
            Ok(layering::install_packages(&refs))
        }
        "RemovePackages" => {
            let pkgs = str_array_or_empty(params, "packages")?;
            let validated: Vec<String> = pkgs
                .iter()
                .map(|p| validated_safe_arg(p, "packages"))
                .collect::<Result<_, _>>()?;
            let refs: Vec<&str> = validated.iter().map(String::as_str).collect();
            Ok(layering::remove_packages(&refs))
        }
        "AddLayeredPackage" => {
            let package = validated_safe_arg(require_str(params, "package")?, "package")?;
            Ok(layering::add_layered_package(&package))
        }
        "RemoveLayeredPackage" => {
            let package = validated_safe_arg(require_str(params, "package")?, "package")?;
            Ok(layering::remove_layered_package(&package))
        }
        "ReplaceLayeredPackage" => {
            let old = validated_safe_arg(require_str(params, "old")?, "old")?;
            let new = validated_safe_arg(require_str(params, "new")?, "new")?;
            Ok(layering::replace_layered_package(&old, &new))
        }

        // ── Package repositories ──────────────────────────────────────────
        "ListPackageRepositories" => Ok(package_repos::list_package_repositories()),
        "AddPackageRepository" => Ok(package_repos::add_package_repository(
            validated_repo_id(params)?,
            validated_no_newline(params, "repo_url")?,
        )),
        "RemovePackageRepository" => Ok(package_repos::remove_package_repository(
            validated_repo_id(params)?,
        )),
        "EnablePackageRepository" => Ok(package_repos::enable_package_repository(
            validated_repo_id(params)?,
        )),
        "DisablePackageRepository" => Ok(package_repos::disable_package_repository(
            validated_repo_id(params)?,
        )),

        // ── Services ─────────────────────────────────────────────────────
        "ListServices" => Ok(services::list_services()),
        "StartService" => {
            let unit = validated_unit_name(require_str(params, "unit")?, "unit")?;
            Ok(services::start_service(&unit))
        }
        "StopService" => {
            let unit = validated_unit_name(require_str(params, "unit")?, "unit")?;
            Ok(services::stop_service(&unit))
        }
        "RestartService" => {
            let unit = validated_unit_name(require_str(params, "unit")?, "unit")?;
            Ok(services::restart_service(&unit))
        }
        "SetServiceEnabled" => {
            let unit = validated_unit_name(require_str(params, "unit")?, "unit")?;
            Ok(services::set_service_enabled(
                &unit,
                require_bool(params, "enabled")?,
            ))
        }
        "MaskService" => {
            let unit = validated_unit_name(require_str(params, "unit")?, "unit")?;
            Ok(services::mask_service(&unit))
        }
        "UnmaskService" => {
            let unit = validated_unit_name(require_str(params, "unit")?, "unit")?;
            Ok(services::unmask_service(&unit))
        }
        "GetServiceLogs" => {
            let unit = validated_unit_name(require_str(params, "unit")?, "unit")?;
            Ok(services::get_service_logs(&unit))
        }

        // ── Toolbox ───────────────────────────────────────────────────────
        "ListToolboxes" => Ok(toolbox::list_toolboxes()),
        "CreateToolbox" => {
            let name = validated_safe_arg(require_str(params, "name")?, "name")?;
            let image = match params.get("image").and_then(|v| v.as_str()) {
                Some(img) => Some(validated_safe_arg(img, "image")?),
                None => None,
            };
            Ok(toolbox::create_toolbox(
                &name,
                params.get("release").and_then(|v| v.as_str()),
                image.as_deref(),
            ))
        }
        "RemoveToolbox" => {
            let name = validated_safe_arg(require_str(params, "name")?, "name")?;
            Ok(toolbox::remove_toolbox(&name))
        }

        // ── Identity ─────────────────────────────────────────────────────
        "SetHostname" => {
            let hostname = validated_hostname(require_str(params, "hostname")?, "hostname")?;
            Ok(identity::set_hostname(&hostname))
        }
        "SetTimezone" => {
            let timezone = validated_timezone(require_str(params, "timezone")?, "timezone")?;
            Ok(identity::set_timezone(&timezone))
        }
        "SetLocale" => {
            let locale = validated_locale(require_str(params, "locale")?, "locale")?;
            Ok(identity::set_locale(&locale))
        }
        "SetNtp" => Ok(identity::set_ntp(require_bool(params, "enabled")?)),

        // ── Filesystem ────────────────────────────────────────────────────
        "GetDiskUsage" => Ok(filesystem::disk_usage_spec()),

        // ── Processes ────────────────────────────────────────────────────
        "ListProcesses" => Ok(processes::list_processes_spec()),

        // ── System info ──────────────────────────────────────────────────
        "GetMemoryInfo" => Ok(system_info::get_memory_info_spec()),

        // ── Network ───────────────────────────────────────────────────────
        "GetFirewallState" => Ok(network::get_firewall_state()),
        "GetNetworkStatus" => Ok(network::get_network_status()),
        "ConfigureWifi" => {
            let ssid = validated_safe_arg(require_str(params, "ssid")?, "ssid")?;
            Ok(network::configure_wifi(&ssid))
        }
        "SetDnsServers" => {
            let interface = validated_safe_arg(require_str(params, "interface")?, "interface")?;
            let servers = str_array_or_empty(params, "servers")?;
            let refs: Vec<&str> = servers.iter().map(String::as_str).collect();
            Ok(network::set_dns_servers(&interface, &refs))
        }
        "ConfigureFirewall" => {
            let zone = validated_safe_arg(require_str(params, "zone")?, "zone")?;
            let service = validated_safe_arg(require_str(params, "service")?, "service")?;
            Ok(network::configure_firewall(
                &zone,
                &service,
                require_bool(params, "enabled")?,
            ))
        }

        // ── Users ─────────────────────────────────────────────────────────
        "ListUsers" => Ok(users::list_users()),
        "ListGroups" => Ok(users::list_groups()),
        "CreateUser" => {
            let username = validated_username(require_str(params, "username")?, "username")?;
            Ok(users::create_user(
                &username,
                params.get("shell").and_then(|v| v.as_str()),
                params.get("home").and_then(|v| v.as_str()),
            ))
        }
        "DeleteUser" => {
            let username = validated_username(require_str(params, "username")?, "username")?;
            Ok(users::delete_user(&username))
        }
        "AddUserToGroup" => {
            let username = validated_username(require_str(params, "username")?, "username")?;
            let group = validated_group(require_str(params, "group")?, "group")?;
            Ok(users::add_user_to_group(&username, &group))
        }
        "RemoveUserFromGroup" => {
            let username = validated_username(require_str(params, "username")?, "username")?;
            let group = validated_group(require_str(params, "group")?, "group")?;
            Ok(users::remove_user_from_group(&username, &group))
        }

        // ── SSH ──────────────────────────────────────────────────────────
        "GetAuthorizedKeys" => {
            let username = validated_username(require_str(params, "username")?, "username")?;
            Ok(ssh::get_authorized_keys(&username))
        }
        "AddAuthorizedKey" => {
            let username = validated_username(require_str(params, "username")?, "username")?;
            let public_key = validated_public_key(require_str(params, "public_key")?)?;
            Ok(ssh::add_authorized_key(&username, &public_key))
        }
        "RemoveAuthorizedKey" => {
            let username = validated_username(require_str(params, "username")?, "username")?;
            let public_key = validated_public_key(require_str(params, "public_key")?)?;
            Ok(ssh::remove_authorized_key(&username, &public_key))
        }

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
            if patched == content && !search.is_empty() {
                return Ok(ExecutionOutput {
                    stdout: String::new(),
                    stderr: format!("search string not found in file: {}", path),
                    exit_code: 1,
                });
            }
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

/// Validate an SSH public key: must start with a known key-type prefix,
/// contain only printable ASCII, no newlines, no single quotes (to prevent
/// shell injection in `sh -c` scripts), and be at most 8192 characters.
fn validated_public_key(s: &str) -> Result<String, ExecutorError> {
    const MAX_LEN: usize = 8192;
    const ALLOWED_PREFIXES: &[&str] = &[
        "ssh-rsa",
        "ssh-ed25519",
        "ssh-ed25519-sk",
        "ecdsa-sha2-nistp256",
        "ecdsa-sha2-nistp384",
        "ecdsa-sha2-nistp521",
        "sk-ssh-ed25519",
        "sk-ecdsa-sha2-nistp256",
    ];

    if s.is_empty() || s.len() > MAX_LEN {
        return Err(ExecutorError::InvalidParam("public_key"));
    }
    if !ALLOWED_PREFIXES.iter().any(|p| s.starts_with(p)) {
        return Err(ExecutorError::InvalidParam("public_key"));
    }
    // No newlines, no single quotes, only printable ASCII.
    if s.chars()
        .any(|c| c == '\n' || c == '\r' || c == '\'' || !c.is_ascii() || c.is_ascii_control())
    {
        return Err(ExecutorError::InvalidParam("public_key"));
    }
    Ok(s.to_string())
}

fn require_bool(params: &Value, key: &'static str) -> Result<bool, ExecutorError> {
    params
        .get(key)
        .and_then(|v| v.as_bool())
        .ok_or(ExecutorError::MissingParam(key))
}

fn require_u32(params: &Value, key: &'static str) -> Result<u32, ExecutorError> {
    let n = params
        .get(key)
        .and_then(|v| v.as_u64())
        .ok_or(ExecutorError::MissingParam(key))?;
    u32::try_from(n).map_err(|_| ExecutorError::InvalidParam(key))
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
/// Only the rpm-ostree deployment and layering actions support rollback —
/// they all revert via `rpm-ostree rollback`. All other actions either have
/// no sensible rollback or are low-risk enough that a rollback would be
/// net-harmful.
///
/// `RollbackDeployment` itself is excluded to prevent infinite recursion.
pub fn rollback_spec_for(action_name: &str) -> Option<ActionSpec> {
    match action_name {
        "UpdateSystem"
        | "InstallPackages"
        | "RemovePackages"
        | "RebaseSystem"
        | "SetKernelArguments"
        | "AddLayeredPackage"
        | "RemoveLayeredPackage"
        | "ReplaceLayeredPackage"
        | "ResetLayeredPackageOverride" => Some(ActionSpec {
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
    fn require_u32_rejects_overflow() {
        let err = build_action_spec("PinDeployment", &json!({ "index": u64::MAX })).unwrap_err();
        assert!(
            matches!(err, ExecutorError::InvalidParam("index")),
            "expected InvalidParam(index), got: {err}"
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
    async fn execute_spec_file_patch_returns_error_when_search_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("repo.conf").to_string_lossy().into_owned();
        std::fs::write(&path, "[myrepo]\nenabled=1\n").unwrap();
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
        let out = execute_spec(&spec).await.unwrap();
        assert_eq!(out.exit_code, 1, "should fail when search string is absent");
        assert!(
            out.stderr.contains("search string not found in file"),
            "stderr should explain the failure: {}",
            out.stderr
        );
        // File should remain unchanged.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[myrepo]\nenabled=1\n"
        );
    }

    #[tokio::test]
    async fn execute_spec_file_patch_allows_empty_search_string() {
        // An empty search string triggers replacen's prepend behavior and should
        // not be rejected — the caller explicitly asked for a no-op search.
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.txt").to_string_lossy().into_owned();
        std::fs::write(&path, "hello").unwrap();
        let spec = ActionSpec {
            action_name: "Test",
            mechanism: ActionMechanism::FilePatch {
                path: path.clone(),
                search: String::new(),
                replace: "prefix-".to_string(),
            },
            risk_level: RiskLevel::Low,
            reboot_required: false,
            rollback_available: false,
        };
        let out = execute_spec(&spec).await.unwrap();
        assert_eq!(out.exit_code, 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "prefix-hello");
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

    /// Every action that claims `rollback_available: true` MUST have a
    /// corresponding entry in `rollback_spec_for()`; every action that claims
    /// `false` MUST NOT. This prevents the spec and the executor from
    /// drifting apart.
    #[test]
    fn rollback_available_matches_rollback_spec_for_all_actions() {
        let all_specs: Vec<ActionSpec> = containers::specs()
            .into_iter()
            .chain(deployment::specs())
            .chain(filesystem::specs())
            .chain(flatpak::specs())
            .chain(identity::specs())
            .chain(layering::specs())
            .chain(network::specs())
            .chain(package_repos::specs())
            .chain(processes::specs())
            .chain(services::specs())
            .chain(ssh::specs())
            .chain(system_info::specs())
            .chain(toolbox::specs())
            .chain(users::specs())
            .collect();

        for spec in &all_specs {
            let has_rollback = rollback_spec_for(spec.action_name).is_some();
            assert_eq!(
                spec.rollback_available,
                has_rollback,
                "action {:?}: rollback_available={} but rollback_spec_for returns {}",
                spec.action_name,
                spec.rollback_available,
                if has_rollback { "Some" } else { "None" },
            );
        }
    }
}
