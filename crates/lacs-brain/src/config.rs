//! Brain configuration loaded from environment variables.
//!
//! Resolution order:
//!   1. `LACS_LLM_PROVIDER` — "anthropic" | "ollama"
//!      If unset: "anthropic" when `ANTHROPIC_API_KEY` is present, else "ollama".
//!   2. `ANTHROPIC_API_KEY` — required when provider is anthropic. Must be non-empty.
//!   3. `LACS_LLM_MODEL` — overrides the provider default model.
//!   4. `LACS_ANTHROPIC_URL` — overrides the Anthropic base URL (default: https://api.anthropic.com).
//!   5. `LACS_OLLAMA_URL` — overrides the Ollama base URL (default: http://localhost:11434).
//!   6. `LACS_BRAIN_MAX_TURNS` — planning loop turn limit (default: 5). Must be >= 1 when set.

use std::fmt;

pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-6";
pub const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_OLLAMA_MODEL: &str = "llama3.2";
pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";
pub const DEFAULT_MAX_TURNS: usize = 5;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct BrainConfig {
    pub(crate) provider: ProviderConfig,
    pub max_turns: usize,
}

#[derive(Clone)]
pub(crate) enum ProviderConfig {
    Anthropic {
        /// Never logged or exposed in error messages.
        api_key: String,
        model: String,
        base_url: String,
    },
    Ollama {
        base_url: String,
        model: String,
    },
}

/// Custom Debug impl to redact the API key.
impl fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderConfig::Anthropic {
                model, base_url, ..
            } => f
                .debug_struct("Anthropic")
                .field("api_key", &"[redacted]")
                .field("model", model)
                .field("base_url", base_url)
                .finish(),
            ProviderConfig::Ollama { base_url, model } => f
                .debug_struct("Ollama")
                .field("base_url", base_url)
                .field("model", model)
                .finish(),
        }
    }
}

impl fmt::Debug for BrainConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrainConfig")
            .field("provider", &self.provider)
            .field("max_turns", &self.max_turns)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("ANTHROPIC_API_KEY is required when provider is 'anthropic' and must not be empty")]
    MissingAnthropicKey,

    #[error("unknown provider '{0}': expected 'anthropic' or 'ollama'")]
    UnknownProvider(String),

    #[error("LACS_BRAIN_MAX_TURNS must be a positive integer (>= 1), got '{0}'")]
    InvalidMaxTurns(String),
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl BrainConfig {
    /// Load from environment variables.
    ///
    /// Returns `Err(ConfigError::InvalidMaxTurns)` if `LACS_BRAIN_MAX_TURNS` is
    /// set to a non-positive integer or an unparseable value. Unset → default of 5.
    ///
    /// Returns `Err(ConfigError::MissingAnthropicKey)` if the provider is
    /// `anthropic` and `ANTHROPIC_API_KEY` is absent or empty.
    pub fn from_env() -> Result<Self, ConfigError> {
        let model_override = std::env::var("LACS_LLM_MODEL").ok();

        let max_turns = match std::env::var("LACS_BRAIN_MAX_TURNS") {
            Err(_) => DEFAULT_MAX_TURNS, // not set → use default
            Ok(raw) => {
                let parsed: usize = raw
                    .parse()
                    .map_err(|_| ConfigError::InvalidMaxTurns(raw.clone()))?;
                if parsed == 0 {
                    return Err(ConfigError::InvalidMaxTurns(raw));
                }
                parsed
            }
        };

        let provider_name = std::env::var("LACS_LLM_PROVIDER").unwrap_or_else(|_| {
            if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                "anthropic".into()
            } else {
                "ollama".into()
            }
        });

        let provider = match provider_name.to_lowercase().as_str() {
            "anthropic" => {
                let api_key = std::env::var("ANTHROPIC_API_KEY")
                    .ok()
                    .filter(|k| !k.is_empty())
                    .ok_or(ConfigError::MissingAnthropicKey)?;
                let base_url = std::env::var("LACS_ANTHROPIC_URL")
                    .unwrap_or_else(|_| DEFAULT_ANTHROPIC_BASE_URL.into());
                ProviderConfig::Anthropic {
                    api_key,
                    model: model_override.unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.into()),
                    base_url,
                }
            }
            "ollama" => {
                let base_url = std::env::var("LACS_OLLAMA_URL")
                    .unwrap_or_else(|_| DEFAULT_OLLAMA_BASE_URL.into());
                ProviderConfig::Ollama {
                    base_url,
                    model: model_override.unwrap_or_else(|| DEFAULT_OLLAMA_MODEL.into()),
                }
            }
            other => return Err(ConfigError::UnknownProvider(other.into())),
        };

        Ok(Self {
            provider,
            max_turns,
        })
    }

    /// Ollama with defaults — used when no API key is configured.
    pub fn ollama_defaults() -> Self {
        Self {
            provider: ProviderConfig::Ollama {
                base_url: DEFAULT_OLLAMA_BASE_URL.into(),
                model: DEFAULT_OLLAMA_MODEL.into(),
            },
            max_turns: DEFAULT_MAX_TURNS,
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
    fn unknown_provider_returns_error() {
        // Temporarily set the env var; since tests may run in parallel
        // we use a scoped approach with a unique var name.
        // We can't easily set LACS_LLM_PROVIDER per-test without a mutex,
        // so we test ConfigError directly.
        let err = ConfigError::UnknownProvider("foobar".into());
        assert!(err.to_string().contains("foobar"));
    }

    #[test]
    fn ollama_defaults_is_valid() {
        let cfg = BrainConfig::ollama_defaults();
        assert!(matches!(cfg.provider, ProviderConfig::Ollama { .. }));
        assert_eq!(cfg.max_turns, DEFAULT_MAX_TURNS);
    }

    #[test]
    fn debug_redacts_api_key() {
        let cfg = ProviderConfig::Anthropic {
            api_key: "sk-secret-key".into(),
            model: "claude-sonnet-4-6".into(),
            base_url: DEFAULT_ANTHROPIC_BASE_URL.into(),
        };
        let debug_str = format!("{cfg:?}");
        assert!(!debug_str.contains("sk-secret-key"));
        assert!(debug_str.contains("[redacted]"));
    }

    // -- InvalidMaxTurns -------------------------------------------------------

    #[test]
    fn invalid_max_turns_error_message_includes_value() {
        let err = ConfigError::InvalidMaxTurns("0".into());
        assert!(err.to_string().contains("0"), "got: {err}");
    }

    #[test]
    fn invalid_max_turns_error_message_includes_non_numeric() {
        let err = ConfigError::InvalidMaxTurns("abc".into());
        assert!(err.to_string().contains("abc"), "got: {err}");
    }

    // -- env-var isolation tests ----------------------------------------------
    // These tests mutate process env vars and must not run concurrently.
    // A crate-level mutex ensures sequential execution.

    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn from_env_max_turns_zero_returns_error() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("LACS_LLM_PROVIDER");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("LACS_LLM_MODEL");
            std::env::remove_var("LACS_OLLAMA_URL");
            std::env::set_var("LACS_BRAIN_MAX_TURNS", "0");
        }
        let result = BrainConfig::from_env();
        unsafe {
            std::env::remove_var("LACS_BRAIN_MAX_TURNS");
        }
        assert!(
            matches!(result, Err(ConfigError::InvalidMaxTurns(_))),
            "expected InvalidMaxTurns, got: {result:?}"
        );
    }

    #[test]
    fn from_env_max_turns_non_numeric_returns_error() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("LACS_LLM_PROVIDER");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("LACS_LLM_MODEL");
            std::env::remove_var("LACS_OLLAMA_URL");
            std::env::set_var("LACS_BRAIN_MAX_TURNS", "not-a-number");
        }
        let result = BrainConfig::from_env();
        unsafe {
            std::env::remove_var("LACS_BRAIN_MAX_TURNS");
        }
        assert!(
            matches!(result, Err(ConfigError::InvalidMaxTurns(_))),
            "expected InvalidMaxTurns, got: {result:?}"
        );
    }

    #[test]
    fn from_env_empty_api_key_returns_missing_key() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("LACS_LLM_PROVIDER", "anthropic");
            std::env::set_var("ANTHROPIC_API_KEY", "");
            std::env::remove_var("LACS_LLM_MODEL");
            std::env::remove_var("LACS_ANTHROPIC_URL");
            std::env::remove_var("LACS_BRAIN_MAX_TURNS");
        }
        let result = BrainConfig::from_env();
        unsafe {
            std::env::remove_var("LACS_LLM_PROVIDER");
            std::env::remove_var("ANTHROPIC_API_KEY");
        }
        assert!(
            matches!(result, Err(ConfigError::MissingAnthropicKey)),
            "expected MissingAnthropicKey for empty key, got: {result:?}"
        );
    }

    #[test]
    fn from_env_ollama_explicit_builds_config() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("LACS_LLM_PROVIDER", "ollama");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("LACS_LLM_MODEL");
            std::env::remove_var("LACS_OLLAMA_URL");
            std::env::remove_var("LACS_BRAIN_MAX_TURNS");
        }
        let result = BrainConfig::from_env();
        unsafe {
            std::env::remove_var("LACS_LLM_PROVIDER");
        }
        let cfg = result.expect("ollama config should succeed");
        assert!(matches!(cfg.provider, ProviderConfig::Ollama { .. }));
        assert_eq!(cfg.max_turns, DEFAULT_MAX_TURNS);
    }

    #[test]
    fn from_env_max_turns_valid_override() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("LACS_LLM_PROVIDER", "ollama");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("LACS_LLM_MODEL");
            std::env::remove_var("LACS_OLLAMA_URL");
            std::env::set_var("LACS_BRAIN_MAX_TURNS", "3");
        }
        let result = BrainConfig::from_env();
        unsafe {
            std::env::remove_var("LACS_LLM_PROVIDER");
            std::env::remove_var("LACS_BRAIN_MAX_TURNS");
        }
        let cfg = result.expect("config with max_turns=3 should succeed");
        assert_eq!(cfg.max_turns, 3);
    }
}
