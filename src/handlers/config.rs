use anyhow::{Context, Result, anyhow};

use crate::cli::{ConfigArgs, ConfigSetArgs, ConfigSubcommands};
use crate::utils::config::{self, ConfigKey};

pub(crate) fn handle(args: ConfigArgs, _json: bool) -> Result<()> {
    match args.command {
        ConfigSubcommands::List => handle_list(),
        ConfigSubcommands::Get(args) => handle_get(&args.key),
        ConfigSubcommands::Set(args) => handle_set(args),
    }
}

fn handle_list() -> Result<()> {
    let current = config::load_config();
    for key in config::all_config_keys() {
        match current.value_for_key(*key) {
            Some(value) => println!("{}={}", key.as_str(), value),
            None => println!("{}=<unset>", key.as_str()),
        }
    }
    Ok(())
}

fn handle_get(key: &str) -> Result<()> {
    let key = parse_known_key(key)?;
    let current = config::load_config();
    match current.value_for_key(key) {
        Some(value) => println!("{}", value),
        None => println!("<unset>"),
    }
    Ok(())
}

fn handle_set(args: ConfigSetArgs) -> Result<()> {
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
    println!("{}={}", key.as_str(), normalized);
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
