use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use crate::cli::{ConfigArgs, ConfigSetArgs, ConfigSubcommands};
use crate::utils::config::{self, ConfigKey};

pub(crate) fn handle(args: ConfigArgs, json: bool) -> Result<()> {
    match args.command {
        ConfigSubcommands::List => handle_list(json),
        ConfigSubcommands::Get(args) => handle_get(&args.key, json),
        ConfigSubcommands::Set(args) => handle_set(args, json),
    }
}

fn handle_list(json: bool) -> Result<()> {
    let current = config::load_config();

    if json {
        let mut map = BTreeMap::new();
        for key in config::all_config_keys() {
            let value = match current.value_for_key(*key) {
                Some(v) => serde_json::Value::String(v),
                None => serde_json::Value::Null,
            };
            map.insert(key.as_str().to_string(), value);
        }
        crate::output::print_json(&map)?;
    } else {
        for key in config::all_config_keys() {
            match current.value_for_key(*key) {
                Some(value) => println!("{}={}", key.as_str(), value),
                None => println!("{}=<unset>", key.as_str()),
            }
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct ConfigGetOutput {
    key: String,
    value: Option<String>,
}

fn handle_get(key: &str, json: bool) -> Result<()> {
    let key = parse_known_key(key)?;
    let current = config::load_config();
    let value = current.value_for_key(key);

    if json {
        crate::output::print_json(&ConfigGetOutput { key: key.as_str().to_string(), value })?;
    } else {
        match value {
            Some(v) => println!("{}", v),
            None => println!("<unset>"),
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct ConfigSetOutput {
    key: String,
    value: String,
}

fn handle_set(args: ConfigSetArgs, json: bool) -> Result<()> {
    let (raw_key, raw_value) = if let Some(value) = args.value {
        if args.key_or_assignment.contains('=') {
            return Err(anyhow!("Ambiguous input. Use either KEY=VALUE or KEY VALUE format."));
        }
        (args.key_or_assignment, value)
    } else {
        args.key_or_assignment
            .split_once('=')
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .context("Invalid assignment format. Use KEY=VALUE or KEY VALUE.")?
    };

    let key = parse_known_key(raw_key.trim())?;
    let normalized = config::set_config_key_value(key, &raw_value)?;

    if json {
        crate::output::print_json(&ConfigSetOutput {
            key: key.as_str().to_string(),
            value: normalized,
        })?;
    } else {
        println!("{}={}", key.as_str(), normalized);
    }

    Ok(())
}

fn parse_known_key(raw: &str) -> Result<ConfigKey> {
    if raw.is_empty() {
        return Err(anyhow!("Config key cannot be empty."));
    }

    match config::parse_config_key(raw) {
        Some(key) => Ok(key),
        None => {
            let supported = config::supported_config_key_names().join(", ");
            Err(anyhow!("Unknown config key '{}'. Supported keys: {}", raw, supported))
        }
    }
}
