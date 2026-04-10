//! Brain configuration loaded from environment variables.
//!
//! Resolution order:
//!   1. `LACS_LLM_PROVIDER` — "anthropic" | "ollama"
//!      If unset: "anthropic" when `ANTHROPIC_API_KEY` is present, else "ollama".
//!   2. `ANTHROPIC_API_KEY` — required when provider is anthropic.
//!   3. `LACS_LLM_MODEL` — overrides the provider default model.
//!   4. `LACS_OLLAMA_URL` — overrides the Ollama base URL.
//!   5. `LACS_BRAIN_MAX_TURNS` — planning loop turn limit (default: 5).

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
    pub provider: ProviderConfig,
    pub max_turns: usize,
}

#[derive(Clone)]
pub enum ProviderConfig {
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
            ProviderConfig::Anthropic { model, base_url, .. } => f
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
    #[error("ANTHROPIC_API_KEY is required when provider is 'anthropic'")]
    MissingAnthropicKey,

    #[error("unknown provider '{0}': expected 'anthropic' or 'ollama'")]
    UnknownProvider(String),
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl BrainConfig {
    /// Load from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        let model_override = std::env::var("LACS_LLM_MODEL").ok();

        let max_turns = std::env::var("LACS_BRAIN_MAX_TURNS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_TURNS);

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
                    .map_err(|_| ConfigError::MissingAnthropicKey)?;
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

        Ok(Self { provider, max_turns })
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
}
