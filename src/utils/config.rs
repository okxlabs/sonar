//! Configuration file support for sonar.
//!
//! Loads settings from `~/.config/sonar/config.toml` and applies them as
//! environment variables before CLI parsing, so that clap's built-in
//! priority chain (CLI arg > env var > default) is preserved.
//!
//! # Example config
//!
//! ```toml
//! rpc_url = "https://my-custom-rpc.example.com"
//! idl_dir = "~/.sonar/idls"
//! color = "auto"
//! show_balance_change = true
//! show_ix_detail = true
//! ```

use std::path::PathBuf;

use serde::Deserialize;

/// User configuration loaded from `~/.config/sonar/config.toml`.
#[derive(Debug, Deserialize, Default)]
pub struct SonarConfig {
    /// Default Solana RPC URL.  Maps to `RPC_URL` env var.
    pub rpc_url: Option<String>,
    /// Default directory for Anchor IDL JSON files.  Maps to `SONAR_IDL_DIR` env var.
    pub idl_dir: Option<String>,
    /// Default color mode (`auto`, `always`, `never`).  Maps to `SONAR_COLOR` env var.
    pub color: Option<String>,
    /// Default for `--show-balance-change`. Maps to `SONAR_SHOW_BALANCE_CHANGE` env var.
    pub show_balance_change: Option<bool>,
    /// Default for `--show-ix-detail`. Maps to `SONAR_SHOW_IX_DETAIL` env var.
    pub show_ix_detail: Option<bool>,
    /// Default for `--raw-log`. Maps to `SONAR_RAW_LOG` env var.
    pub raw_log: Option<bool>,
    /// Default for `--raw-ix-data`. Maps to `SONAR_RAW_IX_DATA` env var.
    pub raw_ix_data: Option<bool>,
    /// Default for `--check-sig`. Maps to `SONAR_VERIFY_SIGNATURES` env var.
    pub verify_signatures: Option<bool>,
    /// Default for `--skip-preflight`. Maps to `SONAR_SKIP_PREFLIGHT` env var.
    pub skip_preflight: Option<bool>,
}

/// Returns the default config file path: `$HOME/.config/sonar/config.toml`.
fn default_config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()?;
    Some(PathBuf::from(home).join(".config").join("sonar").join("config.toml"))
}

/// Expand a leading `~` to the user's home directory.
pub(crate) fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            return format!("{}/{}", home, rest);
        }
    }
    path.to_string()
}

/// Load config from the default location.
///
/// Returns `SonarConfig::default()` when the file does not exist or cannot be parsed.
pub fn load_config() -> SonarConfig {
    let path = match default_config_path() {
        Some(p) => p,
        None => return SonarConfig::default(),
    };

    if !path.exists() {
        return SonarConfig::default();
    }

    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => {
                log::debug!("Loaded config from {}", path.display());
                config
            }
            Err(e) => {
                eprintln!("Warning: Failed to parse config file {}: {}", path.display(), e);
                SonarConfig::default()
            }
        },
        Err(e) => {
            eprintln!("Warning: Failed to read config file {}: {}", path.display(), e);
            SonarConfig::default()
        }
    }
}

/// Apply config values to environment variables.
///
/// Only sets an env var when it is **not** already set, ensuring that
/// explicit env vars (and therefore CLI args) always take precedence.
///
/// This must be called **before** `Cli::parse()`.
fn apply_config_to_env(config: &SonarConfig) {
    fn set_bool_env(var: &str, value: bool) {
        if std::env::var(var).is_err() {
            std::env::set_var(var, if value { "true" } else { "false" });
        }
    }

    if let Some(ref rpc_url) = config.rpc_url {
        if std::env::var("RPC_URL").is_err() {
            std::env::set_var("RPC_URL", rpc_url);
        }
    }
    if let Some(ref idl_dir) = config.idl_dir {
        if std::env::var("SONAR_IDL_DIR").is_err() {
            std::env::set_var("SONAR_IDL_DIR", expand_tilde(idl_dir));
        }
    }
    if let Some(ref color) = config.color {
        if std::env::var("SONAR_COLOR").is_err() {
            std::env::set_var("SONAR_COLOR", color);
        }
    }
    if let Some(value) = config.show_balance_change {
        set_bool_env("SONAR_SHOW_BALANCE_CHANGE", value);
    }
    if let Some(value) = config.show_ix_detail {
        set_bool_env("SONAR_SHOW_IX_DETAIL", value);
    }
    if let Some(value) = config.raw_log {
        set_bool_env("SONAR_RAW_LOG", value);
    }
    if let Some(value) = config.raw_ix_data {
        set_bool_env("SONAR_RAW_IX_DATA", value);
    }
    if let Some(value) = config.verify_signatures {
        set_bool_env("SONAR_VERIFY_SIGNATURES", value);
    }
    if let Some(value) = config.skip_preflight {
        set_bool_env("SONAR_SKIP_PREFLIGHT", value);
    }
}

/// Load configuration and inject values into the environment.
///
/// Call this once at the very start of `main`, **before** `Cli::parse()`.
pub fn load_and_apply() {
    let config = load_config();
    apply_config_to_env(&config);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_expands_home() {
        // This test relies on HOME being set (standard on Unix/macOS).
        if std::env::var("HOME").is_ok() {
            let expanded = expand_tilde("~/foo/bar");
            assert!(!expanded.starts_with('~'));
            assert!(expanded.ends_with("/foo/bar"));
        }
    }

    #[test]
    fn expand_tilde_no_op_for_absolute_path() {
        let path = "/absolute/path";
        assert_eq!(expand_tilde(path), path);
    }

    #[test]
    fn expand_tilde_no_op_for_relative_path() {
        let path = "relative/path";
        assert_eq!(expand_tilde(path), path);
    }

    #[test]
    fn default_config_path_is_under_home() {
        if std::env::var("HOME").is_ok() {
            let path = default_config_path().expect("HOME is set");
            assert!(path.ends_with(".config/sonar/config.toml"));
        }
    }

    #[test]
    fn deserialize_full_config() {
        let toml_str = r#"
            rpc_url = "https://example.com"
            idl_dir = "~/.sonar/idls"
            color = "never"
            show_balance_change = true
            show_ix_detail = true
            raw_log = false
            raw_ix_data = true
            verify_signatures = false
            skip_preflight = true
        "#;
        let config: SonarConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rpc_url.as_deref(), Some("https://example.com"));
        assert_eq!(config.idl_dir.as_deref(), Some("~/.sonar/idls"));
        assert_eq!(config.color.as_deref(), Some("never"));
        assert_eq!(config.show_balance_change, Some(true));
        assert_eq!(config.show_ix_detail, Some(true));
        assert_eq!(config.raw_log, Some(false));
        assert_eq!(config.raw_ix_data, Some(true));
        assert_eq!(config.verify_signatures, Some(false));
        assert_eq!(config.skip_preflight, Some(true));
    }

    #[test]
    fn deserialize_partial_config() {
        let toml_str = r#"
            rpc_url = "https://example.com"
        "#;
        let config: SonarConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rpc_url.as_deref(), Some("https://example.com"));
        assert!(config.idl_dir.is_none());
        assert!(config.color.is_none());
        assert!(config.show_balance_change.is_none());
        assert!(config.show_ix_detail.is_none());
        assert!(config.raw_log.is_none());
        assert!(config.raw_ix_data.is_none());
        assert!(config.verify_signatures.is_none());
        assert!(config.skip_preflight.is_none());
    }

    #[test]
    fn deserialize_empty_config() {
        let config: SonarConfig = toml::from_str("").unwrap();
        assert!(config.rpc_url.is_none());
        assert!(config.idl_dir.is_none());
        assert!(config.color.is_none());
        assert!(config.show_balance_change.is_none());
        assert!(config.show_ix_detail.is_none());
        assert!(config.raw_log.is_none());
        assert!(config.raw_ix_data.is_none());
        assert!(config.verify_signatures.is_none());
        assert!(config.skip_preflight.is_none());
    }
}
