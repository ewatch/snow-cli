use clap::CommandFactory;
use clap_complete::Shell;

use crate::cli::args::Cli;

pub fn handle(shell: Shell) -> anyhow::Result<()> {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
    Ok(())
}
