//! Config command arguments.

use clap::{Args, Subcommand};

/// View or modify `~/.config/sonar/config.toml`.
#[derive(Args, Debug)]
#[command(after_help = "\
EXAMPLES:
  sonar config list
  sonar config get show_ix_detail
  sonar config set show_ix_detail=true    KEY=VALUE form
  sonar config set show_ix_detail true    KEY VALUE form")]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigSubcommands,
}

#[derive(Subcommand, Debug)]
pub enum ConfigSubcommands {
    /// List all supported config keys and current values
    List,
    /// Get a single config value by key
    Get(ConfigGetArgs),
    /// Set a config value using KEY=VALUE or KEY VALUE
    Set(ConfigSetArgs),
}

#[derive(Args, Debug)]
pub struct ConfigGetArgs {
    /// Config key to read
    #[arg(value_name = "KEY")]
    pub key: String,
}

#[derive(Args, Debug)]
pub struct ConfigSetArgs {
    /// Config key, or KEY=VALUE assignment
    #[arg(value_name = "KEY_OR_ASSIGNMENT")]
    pub key_or_assignment: String,
    /// Config value (optional when using KEY=VALUE form)
    #[arg(value_name = "VALUE", required = false)]
    pub value: Option<String>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Commands};

    #[test]
    fn parses_config_list() {
        let cli = Cli::try_parse_from(["sonar", "config", "list"]).unwrap();
        match cli.command {
            Some(Commands::Config(args)) => {
                assert!(matches!(args.command, super::ConfigSubcommands::List));
            }
            _ => panic!("expected config command"),
        }
    }

    #[test]
    fn rejects_removed_cnofig_alias() {
        let parse_result = Cli::try_parse_from(["sonar", "cnofig", "list"]);
        assert!(parse_result.is_err(), "expected removed alias `cnofig` to be rejected");
    }

    #[test]
    fn parses_config_get() {
        let cli = Cli::try_parse_from(["sonar", "config", "get", "show_ix_detail"]).unwrap();
        match cli.command {
            Some(Commands::Config(args)) => match args.command {
                super::ConfigSubcommands::Get(get) => {
                    assert_eq!(get.key, "show_ix_detail");
                }
                _ => panic!("expected config get command"),
            },
            _ => panic!("expected config command"),
        }
    }

    #[test]
    fn parses_config_set() {
        let cli = Cli::try_parse_from(["sonar", "config", "set", "show_ix_detail=true"]).unwrap();
        match cli.command {
            Some(Commands::Config(args)) => match args.command {
                super::ConfigSubcommands::Set(set) => {
                    assert_eq!(set.key_or_assignment, "show_ix_detail=true");
                    assert_eq!(set.value, None);
                }
                _ => panic!("expected config set command"),
            },
            _ => panic!("expected config command"),
        }
    }

    #[test]
    fn parses_config_set_key_value_form() {
        let cli =
            Cli::try_parse_from(["sonar", "config", "set", "show_ix_detail", "true"]).unwrap();
        match cli.command {
            Some(Commands::Config(args)) => match args.command {
                super::ConfigSubcommands::Set(set) => {
                    assert_eq!(set.key_or_assignment, "show_ix_detail");
                    assert_eq!(set.value.as_deref(), Some("true"));
                }
                _ => panic!("expected config set command"),
            },
            _ => panic!("expected config command"),
        }
    }
}
