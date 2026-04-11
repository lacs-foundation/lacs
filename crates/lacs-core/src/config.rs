//! `~/.config/lacs/config.toml` — optional user configuration file.
//!
//! # Resolution order (highest priority wins)
//!
//! 1. Environment variables (always win — set by the caller or systemd unit)
//! 2. Values in `~/.config/lacs/config.toml`
//! 3. Built-in defaults (defined in `lacs-brain` and `lacs-core`)
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
//! use lacs_core::config::LacsConfig;
//!
//! LacsConfig::load().apply_defaults_to_env();
//! ```
//!
//! # Example `config.toml`
//!
//! ```toml
//! [daemon]
//! socket   = "/run/lacs/daemon.sock"   # written as a raw path, not a URI
//! database = "/var/lib/lacs/daemon.sqlite"
//!
//! [llm]
//! provider     = "ollama"              # "ollama" | "anthropic"
//! model        = "llama3.2"
//! ollama_url   = "http://localhost:11434"
//! max_turns    = 5
//! ```

use std::path::PathBuf;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Top-level structure of `~/.config/lacs/config.toml`.
///
/// All fields are optional — absent sections use built-in defaults.
#[derive(Debug, Default, Deserialize)]
pub struct LacsConfig {
    pub daemon: Option<DaemonSection>,
    pub llm: Option<LlmSection>,
}

/// `[daemon]` section.
#[derive(Debug, Deserialize)]
pub struct DaemonSection {
    /// Unix socket path (raw path, not a URI). Maps to `LACS_LISTEN_URI`.
    /// The loader prepends `unix://` when setting the env var.
    pub socket: Option<String>,
    /// SQLite database path. Maps to `LACS_DATABASE_PATH`.
    pub database: Option<String>,
}

/// `[llm]` section.
#[derive(Debug, Deserialize)]
pub struct LlmSection {
    /// LLM provider: `"ollama"` or `"anthropic"`. Maps to `LACS_LLM_PROVIDER`.
    pub provider: Option<String>,
    /// Model identifier. Maps to `LACS_LLM_MODEL`.
    pub model: Option<String>,
    /// Ollama base URL. Maps to `LACS_OLLAMA_URL`.
    pub ollama_url: Option<String>,
    /// Anthropic base URL. Maps to `LACS_ANTHROPIC_URL`.
    pub anthropic_url: Option<String>,
    /// Planning loop turn limit. Maps to `LACS_BRAIN_MAX_TURNS`.
    pub max_turns: Option<u32>,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl LacsConfig {
    /// Load `~/.config/lacs/config.toml`.
    ///
    /// Returns `LacsConfig::default()` (all `None`) if the file is absent,
    /// unreadable, or unparseable. Errors are silently ignored so that a
    /// broken config file does not prevent the daemon or shell from starting —
    /// the built-in defaults apply instead.
    pub fn load() -> Self {
        let path = config_path();
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!(
                "[lacs] warning: could not parse {}: {e}; using defaults",
                path.display()
            );
            Self::default()
        })
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
                // Accept a raw path like `/run/lacs/daemon.sock` and convert
                // to the URI format the daemon expects.
                let uri = if socket.starts_with("unix://") {
                    socket.clone()
                } else {
                    format!("unix://{socket}")
                };
                set_if_absent("LACS_LISTEN_URI", &uri);
            }
            if let Some(db) = &daemon.database {
                set_if_absent("LACS_DATABASE_PATH", db);
            }
        }

        if let Some(llm) = &self.llm {
            if let Some(provider) = &llm.provider {
                set_if_absent("LACS_LLM_PROVIDER", provider);
            }
            if let Some(model) = &llm.model {
                set_if_absent("LACS_LLM_MODEL", model);
            }
            if let Some(url) = &llm.ollama_url {
                set_if_absent("LACS_OLLAMA_URL", url);
            }
            if let Some(url) = &llm.anthropic_url {
                set_if_absent("LACS_ANTHROPIC_URL", url);
            }
            if let Some(turns) = llm.max_turns {
                set_if_absent("LACS_BRAIN_MAX_TURNS", &turns.to_string());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the path to `~/.config/lacs/config.toml`, respecting
/// `XDG_CONFIG_HOME` if set.
fn config_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("lacs").join("config.toml")
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
        // XDG_CONFIG_HOME pointing to a temp dir with no lacs/config.toml
        let dir = tempfile::tempdir().unwrap();
        // Temporarily override XDG_CONFIG_HOME in this process.
        // Tests that mutate env vars must not run in parallel — use a mutex.
        let _g = ENV_LOCK.lock().unwrap();
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
        let lacs_dir = dir.path().join("lacs");
        std::fs::create_dir_all(&lacs_dir).unwrap();
        std::fs::write(
            lacs_dir.join("config.toml"),
            r#"
[daemon]
socket   = "/run/lacs/daemon.sock"
database = "/var/lib/lacs/daemon.sqlite"

[llm]
provider     = "ollama"
model        = "llama3.2"
ollama_url   = "http://localhost:11434"
max_turns    = 7
"#,
        )
        .unwrap();

        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
        }
        let cfg = LacsConfig::load();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        let daemon = cfg.daemon.expect("daemon section missing");
        assert_eq!(daemon.socket.as_deref(), Some("/run/lacs/daemon.sock"));
        assert_eq!(
            daemon.database.as_deref(),
            Some("/var/lib/lacs/daemon.sqlite")
        );

        let llm = cfg.llm.expect("llm section missing");
        assert_eq!(llm.provider.as_deref(), Some("ollama"));
        assert_eq!(llm.model.as_deref(), Some("llama3.2"));
        assert_eq!(llm.max_turns, Some(7));
    }

    #[test]
    fn apply_defaults_does_not_override_existing_env() {
        let dir = tempfile::tempdir().unwrap();
        let lacs_dir = dir.path().join("lacs");
        std::fs::create_dir_all(&lacs_dir).unwrap();
        std::fs::write(
            lacs_dir.join("config.toml"),
            r#"
[llm]
provider = "anthropic"
"#,
        )
        .unwrap();

        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
            // Pre-set the env var — config file must NOT override it.
            std::env::set_var("LACS_LLM_PROVIDER", "ollama");
        }
        let cfg = LacsConfig::load();
        cfg.apply_defaults_to_env();
        let provider = std::env::var("LACS_LLM_PROVIDER").unwrap();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("LACS_LLM_PROVIDER");
        }
        assert_eq!(provider, "ollama", "env var must win over config file");
    }

    #[test]
    fn socket_path_gets_unix_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let lacs_dir = dir.path().join("lacs");
        std::fs::create_dir_all(&lacs_dir).unwrap();
        std::fs::write(
            lacs_dir.join("config.toml"),
            r#"
[daemon]
socket = "/tmp/test.sock"
"#,
        )
        .unwrap();

        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
            std::env::remove_var("LACS_LISTEN_URI");
        }
        let cfg = LacsConfig::load();
        cfg.apply_defaults_to_env();
        let uri = std::env::var("LACS_LISTEN_URI").unwrap();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("LACS_LISTEN_URI");
        }
        assert_eq!(uri, "unix:///tmp/test.sock");
    }

    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());
}
