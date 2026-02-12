use crate::cli::{Cli, CompletionsArgs};
use clap::CommandFactory;

pub(crate) fn handle(args: CompletionsArgs) {
    let mut cmd = Cli::command();
    clap_complete::generate(args.shell, &mut cmd, "sonar", &mut std::io::stdout());
}
