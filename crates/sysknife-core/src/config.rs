//! `~/.config/sysknife/config.toml` — optional user configuration file.
//!
//! # Resolution order (highest priority wins)
//!
//! 1. Environment variables (always win — set by the caller or systemd unit)
//! 2. Values in `~/.config/sysknife/config.toml`
//! 3. Built-in defaults (defined in `sysknife-brain` and `sysknife-core`))
//!
//! # Usage
//!
//! Call [`LacsConfig::load`] once at startup, then call
//! [`LacsConfig::apply_defaults_to_env`] to populate env vars for any
//! key that the config file sets but that is *not* already present in the
//! environment. After that, the rest of the codebase continues reading env
//! vars as before — no callers need to change.
//!
//! ```no_run
//! use sysknife_core::config::LacsConfig;
//!
//! LacsConfig::load().apply_defaults_to_env();
//! ```
//!
//! # Example `config.toml`
//!
//! ```toml
//! [daemon]
//! socket   = "/run/sysknife/daemon.sock"   # written as a raw path, not a URI
//! database = "/var/lib/sysknife/daemon.sqlite"
//!
//! [llm]
//! provider     = "ollama"              # "ollama" | "anthropic" | "openai" | "gemini" | ...
//! model        = "qwen3:8b"            # default — see sysknife-brain DEFAULT_OLLAMA_MODEL
//! ollama_url   = "http://localhost:11434"
//! max_turns    = 10
//! # Optional: override the auto-detected thinking mode for Ollama.
//! # Default: auto-detect from the model name (qwen3 / qwq / deepseek-r → true).
//! # Set to `false` on CPU-only hosts running thinking models — thinking
//! # traces exceed Ollama's internal request timeout on 4 vCPUs.
//! # ollama_think = false
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Top-level structure of `~/.config/sysknife/config.toml`.
///
/// All fields are optional — absent sections use built-in defaults.
#[derive(Debug, Default, Deserialize)]
pub struct LacsConfig {
    pub daemon: Option<DaemonSection>,
    pub llm: Option<LlmSection>,
    pub policy: Option<PolicySection>,
}

/// `[daemon]` section.
#[derive(Debug, Deserialize)]
pub struct DaemonSection {
    /// Unix socket path (raw path, not a URI). Maps to `SYSKNIFE_LISTEN_URI`.
    /// The loader prepends `unix://` when setting the env var.
    pub socket: Option<String>,
    /// SQLite database path. Maps to `SYSKNIFE_DATABASE_PATH`.
    pub database: Option<String>,
}

/// `[policy]` section. Currently holds per-action risk-level overrides.
///
/// See [`PolicySection::risk_overrides`] for semantics. Absent → no overrides
/// (the daemon uses compile-time defaults from `sysknife-daemon::policy`).
#[derive(Debug, Default, Deserialize)]
pub struct PolicySection {
    /// Per-action risk-level overrides. Map from action name → risk level
    /// (`"Low"` | `"Medium"` | `"High"`). The daemon validates this map at
    /// startup and rejects unknown action names or attempted downgrades.
    ///
    /// Overrides may only **raise** the minimum role required for an action
    /// — never lower it. The compile-time default is a floor.
    ///
    /// Example:
    ///
    /// ```toml
    /// [policy.risk_overrides]
    /// InstallFlatpak = "High"   # require Admin in this org (default: Medium/Dev)
    /// ```
    pub risk_overrides: Option<HashMap<String, String>>,
}

/// `[llm]` section.
#[derive(Debug, Deserialize)]
pub struct LlmSection {
    /// LLM provider: `"ollama"` or `"anthropic"`. Maps to `SYSKNIFE_LLM_PROVIDER`.
    pub provider: Option<String>,
    /// Model identifier. Maps to `SYSKNIFE_LLM_MODEL`.
    pub model: Option<String>,
    /// Ollama base URL. Maps to `SYSKNIFE_OLLAMA_URL`.
    pub ollama_url: Option<String>,
    /// Anthropic base URL. Maps to `SYSKNIFE_ANTHROPIC_URL`.
    pub anthropic_url: Option<String>,
    /// Planning loop turn limit. Maps to `SYSKNIFE_BRAIN_MAX_TURNS`.
    pub max_turns: Option<u32>,
    /// Override Ollama thinking-mode auto-detection. `None` means
    /// `sysknife-brain` decides based on the model name. Maps to
    /// `SYSKNIFE_OLLAMA_THINK` (`"true"` or `"false"`).
    pub ollama_think: Option<bool>,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl LacsConfig {
    /// Returns the path to the config file (`~/.config/sysknife/config.toml`).
    pub fn config_path() -> PathBuf {
        config_path()
    }

    /// Load `~/.config/sysknife/config.toml`.
    ///
    /// Returns `LacsConfig::default()` (all `None`) if the file is absent.
    /// Falls back to defaults on parse error (with a warning). I/O errors
    /// other than `NotFound` (e.g. permission denied) are also warned so the
    /// user knows their config file exists but could not be read.
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                eprintln!(
                    "[sysknife] warning: could not parse {}: {e}; using defaults",
                    path.display()
                );
                Self::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                eprintln!(
                    "[sysknife] warning: could not read {}: {e}; using defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Set environment variables from the config file for any key that is NOT
    /// already present in the process environment.
    ///
    /// # Safety note
    ///
    /// This must be called during single-threaded startup, before the async
    /// runtime or any thread pool is initialised. Modifying env vars while
    /// other threads are reading them is undefined behaviour. Both `main.rs`
    /// (daemon) and the Tauri `setup` hook (shell) satisfy this contract.
    pub fn apply_defaults_to_env(&self) {
        if let Some(daemon) = &self.daemon {
            if let Some(socket) = &daemon.socket {
                // Accept a raw path like `/run/sysknife/daemon.sock` and convert
                // to the URI format the daemon expects.
                let uri = if socket.starts_with("unix://") {
                    socket.clone()
                } else {
                    format!("unix://{socket}")
                };
                set_if_absent("SYSKNIFE_LISTEN_URI", &uri);
            }
            if let Some(db) = &daemon.database {
                set_if_absent("SYSKNIFE_DATABASE_PATH", db);
            }
        }

        if let Some(llm) = &self.llm {
            if let Some(provider) = &llm.provider {
                set_if_absent("SYSKNIFE_LLM_PROVIDER", provider);
            }
            if let Some(model) = &llm.model {
                set_if_absent("SYSKNIFE_LLM_MODEL", model);
            }
            if let Some(url) = &llm.ollama_url {
                set_if_absent("SYSKNIFE_OLLAMA_URL", url);
            }
            if let Some(url) = &llm.anthropic_url {
                set_if_absent("SYSKNIFE_ANTHROPIC_URL", url);
            }
            if let Some(turns) = llm.max_turns {
                set_if_absent("SYSKNIFE_BRAIN_MAX_TURNS", &turns.to_string());
            }
            if let Some(think) = llm.ollama_think {
                // `sysknife-brain::planner::resolve_ollama_think` parses
                // case-insensitive "true"/"false"; emit the canonical
                // form for clarity in ps/systemctl output.
                set_if_absent(
                    "SYSKNIFE_OLLAMA_THINK",
                    if think { "true" } else { "false" },
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `~/.config/sysknife`, respecting `XDG_CONFIG_HOME` if set.
///
/// `XDG_CONFIG_HOME` is accepted only if it is an absolute path with no `..`
/// components, to prevent path-traversal attacks where an attacker sets
/// `XDG_CONFIG_HOME=/etc` to redirect config and prefs writes to system
/// directories. Invalid values are ignored and the default `~/.config` is used.
///
/// If `HOME` is also unset a warning is emitted and `./.config` (relative to
/// CWD) is used as a last resort — callers that write to the config path
/// (daemon, shell) should ensure `HOME` is set at startup.
fn config_dir() -> PathBuf {
    // Validate XDG_CONFIG_HOME: must be absolute and contain no `..` components.
    let xdg_valid = std::env::var("XDG_CONFIG_HOME").ok().and_then(|v| {
        let p = PathBuf::from(&v);
        if p.is_absolute() && !p.components().any(|c| c == std::path::Component::ParentDir) {
            Some(p)
        } else {
            eprintln!(
                "[sysknife] warning: XDG_CONFIG_HOME={v:?} is not a safe absolute path; \
                 ignoring and using default ~/.config"
            );
            None
        }
    });

    let base = xdg_valid.unwrap_or_else(|| match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home).join(".config"),
        Err(_) => {
            eprintln!(
                "[sysknife] warning: HOME is not set; using relative path ./.config \
                     for config and preferences — ensure HOME is set in production"
            );
            PathBuf::from(".config")
        }
    });
    base.join("sysknife")
}

/// Returns the path to `~/.config/sysknife/config.toml`, respecting
/// `XDG_CONFIG_HOME` if set.
pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Returns the path to `~/.config/sysknife/prefs.md`, respecting
/// `XDG_CONFIG_HOME` if set. Same directory as `config.toml`.
pub fn prefs_path() -> PathBuf {
    config_dir().join("prefs.md")
}

/// Set `key` to `value` only if `key` is absent from the process environment.
fn set_if_absent(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        // SAFETY: single-threaded startup — no other threads are reading env
        // vars yet. See `apply_defaults_to_env` safety note.
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(key, value);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_returns_default_when_file_absent() {
        // XDG_CONFIG_HOME pointing to a temp dir with no sysknife/config.toml
        let dir = tempfile::tempdir().unwrap();
        // Temporarily override XDG_CONFIG_HOME in this process.
        // Tests that mutate env vars must not run in parallel — use a mutex.
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
        }
        let cfg = LacsConfig::load();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        assert!(cfg.daemon.is_none());
        assert!(cfg.llm.is_none());
    }

    #[test]
    fn load_parses_full_config() {
        let dir = tempfile::tempdir().unwrap();
        let sysknife_dir = dir.path().join("sysknife");
        std::fs::create_dir_all(&sysknife_dir).unwrap();
        std::fs::write(
            sysknife_dir.join("config.toml"),
            r#"
[daemon]
socket   = "/run/sysknife/daemon.sock"
database = "/var/lib/sysknife/daemon.sqlite"

[llm]
provider     = "ollama"
model        = "llama3.2"
ollama_url   = "http://localhost:11434"
max_turns    = 7
"#,
        )
        .unwrap();

        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
        }
        let cfg = LacsConfig::load();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        let daemon = cfg.daemon.expect("daemon section missing");
        assert_eq!(daemon.socket.as_deref(), Some("/run/sysknife/daemon.sock"));
        assert_eq!(
            daemon.database.as_deref(),
            Some("/var/lib/sysknife/daemon.sqlite")
        );

        let llm = cfg.llm.expect("llm section missing");
        assert_eq!(llm.provider.as_deref(), Some("ollama"));
        assert_eq!(llm.model.as_deref(), Some("llama3.2"));
        assert_eq!(llm.max_turns, Some(7));
    }

    #[test]
    fn apply_defaults_does_not_override_existing_env() {
        let dir = tempfile::tempdir().unwrap();
        let sysknife_dir = dir.path().join("sysknife");
        std::fs::create_dir_all(&sysknife_dir).unwrap();
        std::fs::write(
            sysknife_dir.join("config.toml"),
            r#"
[llm]
provider = "anthropic"
"#,
        )
        .unwrap();

        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
            // Pre-set the env var — config file must NOT override it.
            std::env::set_var("SYSKNIFE_LLM_PROVIDER", "ollama");
        }
        let cfg = LacsConfig::load();
        cfg.apply_defaults_to_env();
        let provider = std::env::var("SYSKNIFE_LLM_PROVIDER").unwrap();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("SYSKNIFE_LLM_PROVIDER");
        }
        assert_eq!(provider, "ollama", "env var must win over config file");
    }

    #[test]
    fn socket_path_gets_unix_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let sysknife_dir = dir.path().join("sysknife");
        std::fs::create_dir_all(&sysknife_dir).unwrap();
        std::fs::write(
            sysknife_dir.join("config.toml"),
            r#"
[daemon]
socket = "/tmp/test.sock"
"#,
        )
        .unwrap();

        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
            std::env::remove_var("SYSKNIFE_LISTEN_URI");
        }
        let cfg = LacsConfig::load();
        cfg.apply_defaults_to_env();
        let uri = std::env::var("SYSKNIFE_LISTEN_URI").unwrap();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("SYSKNIFE_LISTEN_URI");
        }
        assert_eq!(uri, "unix:///tmp/test.sock");
    }

    #[test]
    fn ollama_think_false_emits_env_var() {
        let dir = tempfile::tempdir().unwrap();
        let sysknife_dir = dir.path().join("sysknife");
        std::fs::create_dir_all(&sysknife_dir).unwrap();
        std::fs::write(
            sysknife_dir.join("config.toml"),
            r#"
[llm]
ollama_think = false
"#,
        )
        .unwrap();

        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
            std::env::remove_var("SYSKNIFE_OLLAMA_THINK");
        }
        let cfg = LacsConfig::load();
        cfg.apply_defaults_to_env();
        let think = std::env::var("SYSKNIFE_OLLAMA_THINK").unwrap();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("SYSKNIFE_OLLAMA_THINK");
        }
        assert_eq!(think, "false");
    }

    #[test]
    fn ollama_think_true_emits_env_var() {
        let dir = tempfile::tempdir().unwrap();
        let sysknife_dir = dir.path().join("sysknife");
        std::fs::create_dir_all(&sysknife_dir).unwrap();
        std::fs::write(
            sysknife_dir.join("config.toml"),
            r#"
[llm]
ollama_think = true
"#,
        )
        .unwrap();

        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
            std::env::remove_var("SYSKNIFE_OLLAMA_THINK");
        }
        let cfg = LacsConfig::load();
        cfg.apply_defaults_to_env();
        let think = std::env::var("SYSKNIFE_OLLAMA_THINK").unwrap();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("SYSKNIFE_OLLAMA_THINK");
        }
        assert_eq!(think, "true");
    }

    #[test]
    fn ollama_think_absent_does_not_set_env_var() {
        let dir = tempfile::tempdir().unwrap();
        let sysknife_dir = dir.path().join("sysknife");
        std::fs::create_dir_all(&sysknife_dir).unwrap();
        std::fs::write(
            sysknife_dir.join("config.toml"),
            r#"
[llm]
model = "qwen3:8b"
"#,
        )
        .unwrap();

        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
            std::env::remove_var("SYSKNIFE_OLLAMA_THINK");
        }
        let cfg = LacsConfig::load();
        cfg.apply_defaults_to_env();
        let think_set = std::env::var_os("SYSKNIFE_OLLAMA_THINK").is_some();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        assert!(!think_set, "absent ollama_think must not set the env var");
    }

    #[test]
    fn prefs_path_lives_alongside_config() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prefs = prefs_path();
        let config = config_path();
        assert_eq!(prefs.parent(), config.parent());
        assert_eq!(prefs.file_name().unwrap(), "prefs.md");
    }

    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());
}
