use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::models::Provider;
use crate::models::selector_priorities;

#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    Api,
    CodexCli,
    GeminiCli,
    CursorCli,
    OpenCodeCli,
    ProfileCli(String),
}

impl Backend {
    pub fn from_builtin_str(s: &str) -> Option<Backend> {
        match s {
            "api" => Some(Backend::Api),
            "codex-cli" => Some(Backend::CodexCli),
            "gemini-cli" => Some(Backend::GeminiCli),
            "cursor-cli" => Some(Backend::CursorCli),
            "opencode" => Some(Backend::OpenCodeCli),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Backend::Api => "api",
            Backend::CodexCli => "codex-cli",
            Backend::GeminiCli => "gemini-cli",
            Backend::CursorCli => "cursor-cli",
            Backend::OpenCodeCli => "opencode",
            Backend::ProfileCli(raw) => raw.as_str(),
        }
    }
}

/// Per-provider runtime configuration parsed from environment variables.
#[derive(Debug, Clone)]
pub struct ProviderRuntimeConfig {
    pub api_key: Option<String>,
    pub backend: Backend,
    pub opencode_provider: String,
    pub selected_cli_profile: Option<SelectedCliProfile>,
}

impl ProviderRuntimeConfig {
    /// Returns true if this provider has an available executor for its backend.
    /// For API backends this means an API key is set. For profile-backed backends
    /// it means a matching CLI profile is selected. Built-in CLI backends always
    /// have an executor.
    pub fn has_executable_backend(&self) -> bool {
        match &self.backend {
            Backend::Api => self.api_key.is_some(),
            Backend::ProfileCli(name) => self
                .selected_cli_profile
                .as_ref()
                .is_some_and(|selected| selected.backend == *name && name == "claude-cli"),
            _ => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CliProfileInterface {
    Text,
    Json,
    StreamJson,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CliPromptMode {
    Stdin,
    Argument,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliProfile {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    pub interface: CliProfileInterface,
    pub prompt: CliPromptMode,
    #[serde(default)]
    pub headless: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedCliProfile {
    pub backend: String,
    pub name: String,
    pub profile: CliProfile,
}

#[derive(Debug)]
pub struct Config {
    pub(crate) providers: HashMap<Provider, ProviderRuntimeConfig>,
    #[allow(dead_code)]
    pub default_model: Option<String>,
    pub default_models: Vec<String>,
    pub codex_reasoning_effort: String,
    pub codex_extra_args: Vec<String>,
    pub gemini_extra_args: Vec<String>,
    pub api_idle_timeout: Duration,
    pub system_prompt_path: Option<String>,
    pub allowed_models: Vec<String>,
    #[allow(dead_code)]
    pub cli_profiles: std::collections::BTreeMap<String, CliProfile>,
}

impl Config {
    /// Get the configured backend for a provider.
    pub fn backend_for(&self, provider: Provider) -> &Backend {
        &self.providers[&provider].backend
    }

    /// Get the API key for a provider (when using API backend).
    pub fn api_key_for(&self, provider: Provider) -> Option<&str> {
        self.providers[&provider].api_key.as_deref()
    }

    /// Get the OpenCode provider prefix for a provider family.
    pub fn opencode_provider_for(&self, provider: Provider) -> &str {
        &self.providers[&provider].opencode_provider
    }

    #[allow(dead_code)]
    /// Iterate over all provider runtime configs.
    pub fn iter_providers(&self) -> impl Iterator<Item = (&Provider, &ProviderRuntimeConfig)> {
        self.providers.iter()
    }

    #[allow(dead_code)]
    /// Get the selected CLI profile for a provider, if any.
    pub fn selected_cli_profile_for(&self, provider: Provider) -> Option<&SelectedCliProfile> {
        self.providers[&provider].selected_cli_profile.as_ref()
    }

    #[allow(dead_code)]
    /// Get all configured CLI profiles.
    pub fn all_cli_profiles(&self) -> &std::collections::BTreeMap<String, CliProfile> {
        &self.cli_profiles
    }
}

#[derive(Debug)]
pub enum ConfigError {
    NoModelsAvailable,
    InvalidBackend {
        env_var: String,
        raw: String,
        allowed: Vec<String>,
    },
    InvalidDefaultModel {
        model: String,
        allowed: Vec<String>,
    },
    InvalidDefaultModels {
        model: String,
        allowed: Vec<String>,
    },
    TooManyDefaultModels {
        count: usize,
    },
    InvalidCodexReasoningEffort(String),
    InvalidExtraArgs {
        env_var: String,
        raw: String,
        message: String,
    },
    ConfigFile {
        path: PathBuf,
        message: String,
    },
    MissingCliProfile {
        key: String,
        backend: String,
        allowed: Vec<String>,
    },
    InvalidCliProfileReference {
        key: String,
        raw: String,
        allowed: Vec<String>,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::NoModelsAvailable => write!(
                f,
                "Invalid environment variables:\n  No models available. Set API keys or configure CLI backends."
            ),
            ConfigError::InvalidBackend {
                env_var,
                raw,
                allowed,
            } => {
                let opts = allowed
                    .iter()
                    .map(|v| format!("'{v}'"))
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(
                    f,
                    "Invalid environment variables:\n  {env_var}: Invalid enum value. Expected {opts}, received '{raw}'"
                )
            }
            ConfigError::InvalidDefaultModel { model, allowed } => {
                let selectors: Vec<&str> = selector_priorities().map(|(s, _)| s).collect();
                let opts = allowed
                    .iter()
                    .map(|m| format!("'{m}'"))
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(
                    f,
                    "Invalid environment variables:\n  defaultModel: Invalid value '{model}'. Expected a selector ({}) or exact model ({opts})",
                    selectors.join(", ")
                )
            }
            ConfigError::InvalidDefaultModels { model, allowed } => {
                let selectors: Vec<&str> = selector_priorities().map(|(s, _)| s).collect();
                let opts = allowed
                    .iter()
                    .map(|m| format!("'{m}'"))
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(
                    f,
                    "Invalid environment variables:\n  defaultModels: Invalid value '{model}'. Expected a selector ({}) or exact model ({opts})",
                    selectors.join(", ")
                )
            }
            ConfigError::TooManyDefaultModels { count } => write!(
                f,
                "Invalid environment variables:\n  defaultModels: max 5 total runs, including duplicates (got {count})"
            ),
            ConfigError::InvalidCodexReasoningEffort(effort) => write!(
                f,
                "Invalid environment variables:\n  codexReasoningEffort: Invalid enum value. Expected 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh', received '{effort}'"
            ),
            ConfigError::InvalidExtraArgs {
                env_var,
                raw,
                message,
            } => write!(
                f,
                "Invalid environment variables:\n  {env_var}: {message} (received '{raw}')"
            ),
            ConfigError::ConfigFile { path, message } => write!(
                f,
                "Configuration file error:\n  {}: {}",
                path.display(),
                message,
            ),
            ConfigError::MissingCliProfile {
                key,
                backend,
                allowed,
            } => {
                let opts = allowed
                    .iter()
                    .map(|v| format!("'{v}'"))
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(
                    f,
                    "Invalid environment variables:\n  {key}: CLI profile required for backend '{backend}' but no profile set. Expected one of: {opts}"
                )
            }
            ConfigError::InvalidCliProfileReference { key, raw, allowed } => {
                let opts = allowed
                    .iter()
                    .map(|v| format!("'{v}'"))
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(
                    f,
                    "Invalid environment variables:\n  {key}: Invalid CLI profile '{raw}'. Expected one of: {opts}"
                )
            }
        }
    }
}
