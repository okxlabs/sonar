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
//! no_idl_fetch = false
//! color = "auto"
//! show_balance_change = true
//! show_ix_detail = true
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// User configuration loaded from `~/.config/sonar/config.toml`.
#[derive(Debug, Deserialize, Default)]
pub struct SonarConfig {
    /// Default Solana RPC URL.  Maps to `RPC_URL` env var.
    pub rpc_url: Option<String>,
    /// Default directory for Anchor IDL JSON files.  Maps to `SONAR_IDL_DIR` env var.
    pub idl_dir: Option<String>,
    /// Default for `--no-idl-fetch`. Maps to `SONAR_NO_IDL_FETCH` env var.
    pub no_idl_fetch: Option<bool>,
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ConfigKey {
    RpcUrl,
    IdlDir,
    NoIdlFetch,
    Color,
    ShowBalanceChange,
    ShowIxDetail,
    RawLog,
    RawIxData,
    VerifySignatures,
    SkipPreflight,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ConfigValueKind {
    Bool,
    String,
}

const ALL_CONFIG_KEYS: [ConfigKey; 10] = [
    ConfigKey::RpcUrl,
    ConfigKey::IdlDir,
    ConfigKey::NoIdlFetch,
    ConfigKey::Color,
    ConfigKey::ShowBalanceChange,
    ConfigKey::ShowIxDetail,
    ConfigKey::RawLog,
    ConfigKey::RawIxData,
    ConfigKey::VerifySignatures,
    ConfigKey::SkipPreflight,
];

impl ConfigKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RpcUrl => "rpc_url",
            Self::IdlDir => "idl_dir",
            Self::NoIdlFetch => "no_idl_fetch",
            Self::Color => "color",
            Self::ShowBalanceChange => "show_balance_change",
            Self::ShowIxDetail => "show_ix_detail",
            Self::RawLog => "raw_log",
            Self::RawIxData => "raw_ix_data",
            Self::VerifySignatures => "verify_signatures",
            Self::SkipPreflight => "skip_preflight",
        }
    }

    fn value_kind(self) -> ConfigValueKind {
        match self {
            Self::RpcUrl | Self::IdlDir | Self::Color => ConfigValueKind::String,
            Self::NoIdlFetch
            | Self::ShowBalanceChange
            | Self::ShowIxDetail
            | Self::RawLog
            | Self::RawIxData
            | Self::VerifySignatures
            | Self::SkipPreflight => ConfigValueKind::Bool,
        }
    }
}

impl SonarConfig {
    pub fn value_for_key(&self, key: ConfigKey) -> Option<String> {
        match key {
            ConfigKey::RpcUrl => self.rpc_url.clone(),
            ConfigKey::IdlDir => self.idl_dir.clone(),
            ConfigKey::NoIdlFetch => self.no_idl_fetch.map(|v| v.to_string()),
            ConfigKey::Color => self.color.clone(),
            ConfigKey::ShowBalanceChange => self.show_balance_change.map(|v| v.to_string()),
            ConfigKey::ShowIxDetail => self.show_ix_detail.map(|v| v.to_string()),
            ConfigKey::RawLog => self.raw_log.map(|v| v.to_string()),
            ConfigKey::RawIxData => self.raw_ix_data.map(|v| v.to_string()),
            ConfigKey::VerifySignatures => self.verify_signatures.map(|v| v.to_string()),
            ConfigKey::SkipPreflight => self.skip_preflight.map(|v| v.to_string()),
        }
    }
}

pub fn all_config_keys() -> &'static [ConfigKey] {
    &ALL_CONFIG_KEYS
}

pub fn supported_config_key_names() -> Vec<&'static str> {
    all_config_keys().iter().map(|k| k.as_str()).collect()
}

pub fn parse_config_key(key: &str) -> Option<ConfigKey> {
    match key {
        "rpc_url" => Some(ConfigKey::RpcUrl),
        "idl_dir" => Some(ConfigKey::IdlDir),
        "no_idl_fetch" => Some(ConfigKey::NoIdlFetch),
        "color" => Some(ConfigKey::Color),
        "show_balance_change" => Some(ConfigKey::ShowBalanceChange),
        "show_ix_detail" => Some(ConfigKey::ShowIxDetail),
        "raw_log" => Some(ConfigKey::RawLog),
        "raw_ix_data" => Some(ConfigKey::RawIxData),
        "verify_signatures" => Some(ConfigKey::VerifySignatures),
        "skip_preflight" => Some(ConfigKey::SkipPreflight),
        _ => None,
    }
}

/// Returns the default config file path: `$HOME/.config/sonar/config.toml`.
fn default_config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()?;
    Some(PathBuf::from(home).join(".config").join("sonar").join("config.toml"))
}

/// Returns the default config file path as a fallible API.
pub fn config_path() -> Result<PathBuf> {
    default_config_path().context("Unable to determine home directory for config file path")
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

fn parse_value_for_key(key: ConfigKey, raw_value: &str) -> Result<(toml::Value, String)> {
    match key.value_kind() {
        ConfigValueKind::Bool => {
            let normalized = raw_value.trim().to_ascii_lowercase();
            let parsed = normalized.parse::<bool>().with_context(|| {
                format!("Invalid boolean value for '{}': expected true or false", key.as_str())
            })?;
            Ok((toml::Value::Boolean(parsed), parsed.to_string()))
        }
        ConfigValueKind::String => {
            if key == ConfigKey::Color {
                let normalized = raw_value.trim().to_ascii_lowercase();
                if !matches!(normalized.as_str(), "auto" | "always" | "never") {
                    anyhow::bail!("Invalid value for 'color': expected one of auto, always, never");
                }
                return Ok((toml::Value::String(normalized.clone()), normalized));
            }
            Ok((toml::Value::String(raw_value.to_string()), raw_value.to_string()))
        }
    }
}

fn load_config_table(path: &Path) -> Result<toml::Table> {
    if !path.exists() {
        return Ok(toml::Table::new());
    }

    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file {}", path.display()))?;
    if contents.trim().is_empty() {
        return Ok(toml::Table::new());
    }

    toml::from_str::<toml::Table>(&contents)
        .with_context(|| format!("Failed to parse config file {}", path.display()))
}

fn upsert_config_value_at_path(path: &Path, key: ConfigKey, value: toml::Value) -> Result<()> {
    let mut table = load_config_table(path)?;
    table.insert(key.as_str().to_string(), value);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    }

    let serialized = toml::to_string_pretty(&table)
        .with_context(|| format!("Failed to serialize config file {}", path.display()))?;
    std::fs::write(path, serialized)
        .with_context(|| format!("Failed to write config file {}", path.display()))?;
    Ok(())
}

pub fn set_config_key_value(key: ConfigKey, raw_value: &str) -> Result<String> {
    let path = config_path()?;
    let (value, normalized) = parse_value_for_key(key, raw_value)?;
    upsert_config_value_at_path(&path, key, value)?;
    Ok(normalized)
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
    if let Some(value) = config.no_idl_fetch {
        set_bool_env("SONAR_NO_IDL_FETCH", value);
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
    use std::time::{SystemTime, UNIX_EPOCH};

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
            no_idl_fetch = true
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
        assert_eq!(config.no_idl_fetch, Some(true));
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
        assert!(config.no_idl_fetch.is_none());
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
        assert!(config.no_idl_fetch.is_none());
        assert!(config.color.is_none());
        assert!(config.show_balance_change.is_none());
        assert!(config.show_ix_detail.is_none());
        assert!(config.raw_log.is_none());
        assert!(config.raw_ix_data.is_none());
        assert!(config.verify_signatures.is_none());
        assert!(config.skip_preflight.is_none());
    }

    #[test]
    fn parse_config_key_recognizes_known_key() {
        let key = parse_config_key("show_ix_detail");
        assert_eq!(key, Some(ConfigKey::ShowIxDetail));
    }

    #[test]
    fn parse_value_for_bool_key_rejects_invalid_value() {
        let err = parse_value_for_key(ConfigKey::ShowIxDetail, "yes").unwrap_err();
        assert!(err.to_string().contains("expected true or false"));
    }

    #[test]
    fn parse_value_for_color_enforces_supported_values() {
        let err = parse_value_for_key(ConfigKey::Color, "rainbow").unwrap_err();
        assert!(err.to_string().contains("expected one of auto, always, never"));
    }

    #[test]
    fn upsert_config_value_writes_toml_file() {
        let path = unique_temp_path("write");
        upsert_config_value_at_path(&path, ConfigKey::ShowIxDetail, toml::Value::Boolean(true))
            .unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let table: toml::Table = toml::from_str(&contents).unwrap();
        assert_eq!(table.get("show_ix_detail"), Some(&toml::Value::Boolean(true)));

        cleanup_temp_path(&path);
    }

    #[test]
    fn upsert_config_value_fails_when_existing_toml_is_invalid() {
        let path = unique_temp_path("invalid");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "this is not valid toml = [").unwrap();

        let err =
            upsert_config_value_at_path(&path, ConfigKey::ShowIxDetail, toml::Value::Boolean(true))
                .unwrap_err();
        assert!(err.to_string().contains("Failed to parse config file"));

        cleanup_temp_path(&path);
    }

    fn unique_temp_path(suffix: &str) -> PathBuf {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!(
            "sonar-config-tests-{}-{}-{suffix}/config.toml",
            std::process::id(),
            now
        ))
    }

    fn cleanup_temp_path(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
}
