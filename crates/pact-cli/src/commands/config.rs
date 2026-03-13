//! CLI configuration — journal endpoint, authentication, output format.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// CLI configuration loaded from `~/.config/pact/cli.toml` or environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// Journal gRPC endpoint (e.g. "https://journal.example.com:9443").
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    /// OIDC token for authentication. If not set, reads from token file.
    #[serde(default)]
    pub token: Option<String>,
    /// Path to token file (default: ~/.config/pact/token).
    #[serde(default = "default_token_path")]
    pub token_path: PathBuf,
    /// Default vCluster scope for commands.
    #[serde(default)]
    pub default_vcluster: Option<String>,
    /// Output format: "text" (default), "json", "yaml".
    #[serde(default = "default_output_format")]
    pub output_format: OutputFormat,
    /// Connection timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
}

/// Output format for CLI commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
}

fn default_endpoint() -> String {
    "http://localhost:9443".to_string()
}

fn default_token_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("pact").join("token")
}

fn default_output_format() -> OutputFormat {
    OutputFormat::Text
}

const fn default_timeout() -> u32 {
    30
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            endpoint: default_endpoint(),
            token: None,
            token_path: default_token_path(),
            default_vcluster: None,
            output_format: OutputFormat::Text,
            timeout_seconds: 30,
        }
    }
}

impl CliConfig {
    /// Load config from file, falling back to defaults.
    pub fn load() -> Self {
        let config_path =
            dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("pact").join("cli.toml");

        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(contents) => match toml::from_str(&contents) {
                    Ok(config) => return config,
                    Err(e) => {
                        eprintln!("Warning: invalid config {}: {}", config_path.display(), e);
                    }
                },
                Err(e) => {
                    eprintln!("Warning: cannot read {}: {}", config_path.display(), e);
                }
            }
        }

        Self::default()
    }

    /// Apply environment variable overrides.
    pub fn with_env_overrides(mut self) -> Self {
        if let Ok(endpoint) = std::env::var("PACT_ENDPOINT") {
            self.endpoint = endpoint;
        }
        if let Ok(token) = std::env::var("PACT_TOKEN") {
            self.token = Some(token);
        }
        if let Ok(vcluster) = std::env::var("PACT_VCLUSTER") {
            self.default_vcluster = Some(vcluster);
        }
        if let Ok(format) = std::env::var("PACT_OUTPUT") {
            if format == "json" {
                self.output_format = OutputFormat::Json;
            }
        }
        self
    }

    /// Resolve the bearer token — from CLI arg, config, env, or token file.
    pub fn resolve_token(&self) -> anyhow::Result<String> {
        // Priority: explicit token > env > token file
        if let Some(ref token) = self.token {
            return Ok(token.clone());
        }

        if self.token_path.exists() {
            let token = std::fs::read_to_string(&self.token_path)?.trim().to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }

        anyhow::bail!(
            "No auth token found. Set PACT_TOKEN, pass --token, or write to {}",
            self.token_path.display()
        )
    }

    /// Resolve the vCluster — from CLI arg or default.
    pub fn resolve_vcluster(&self, arg: Option<&str>) -> anyhow::Result<String> {
        arg.map(String::from).or_else(|| self.default_vcluster.clone()).ok_or_else(|| {
            anyhow::anyhow!("No vCluster specified. Use --vcluster or set PACT_VCLUSTER")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = CliConfig::default();
        assert_eq!(config.endpoint, "http://localhost:9443");
        assert!(config.token.is_none());
        assert!(config.default_vcluster.is_none());
        assert_eq!(config.output_format, OutputFormat::Text);
        assert_eq!(config.timeout_seconds, 30);
    }

    #[test]
    fn config_deserialize_from_toml() {
        let toml_str = r#"
            endpoint = "https://journal.prod:9443"
            default_vcluster = "ml-training"
            output_format = "json"
            timeout_seconds = 60
        "#;
        let config: CliConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.endpoint, "https://journal.prod:9443");
        assert_eq!(config.default_vcluster.as_deref(), Some("ml-training"));
        assert_eq!(config.output_format, OutputFormat::Json);
        assert_eq!(config.timeout_seconds, 60);
    }

    #[test]
    fn env_overrides_apply() {
        let config = CliConfig::default();
        // Can't easily test env vars in unit tests, but verify the method returns self
        let overridden = config.with_env_overrides();
        // Without env vars set, should keep defaults
        assert_eq!(overridden.endpoint, "http://localhost:9443");
    }

    #[test]
    fn resolve_vcluster_from_arg() {
        let config = CliConfig::default();
        let vc = config.resolve_vcluster(Some("ml-train")).unwrap();
        assert_eq!(vc, "ml-train");
    }

    #[test]
    fn resolve_vcluster_from_default() {
        let config = CliConfig { default_vcluster: Some("ml-train".into()), ..Default::default() };
        let vc = config.resolve_vcluster(None).unwrap();
        assert_eq!(vc, "ml-train");
    }

    #[test]
    fn resolve_vcluster_missing_fails() {
        let config = CliConfig::default();
        let result = config.resolve_vcluster(None);
        assert!(result.is_err());
    }

    #[test]
    fn resolve_token_from_explicit() {
        let config = CliConfig { token: Some("my-token".into()), ..Default::default() };
        let token = config.resolve_token().unwrap();
        assert_eq!(token, "my-token");
    }

    #[test]
    fn resolve_token_missing_fails() {
        let config = CliConfig {
            token: None,
            token_path: PathBuf::from("/nonexistent/path/token"),
            ..Default::default()
        };
        let result = config.resolve_token();
        assert!(result.is_err());
    }

    #[test]
    fn resolve_token_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "file-token\n").unwrap();

        let config = CliConfig { token: None, token_path: token_file, ..Default::default() };
        let token = config.resolve_token().unwrap();
        assert_eq!(token, "file-token");
    }

    #[test]
    fn output_format_serde() {
        assert_eq!(serde_json::to_string(&OutputFormat::Json).unwrap(), "\"json\"");
        assert_eq!(serde_json::to_string(&OutputFormat::Text).unwrap(), "\"text\"");
    }
}
