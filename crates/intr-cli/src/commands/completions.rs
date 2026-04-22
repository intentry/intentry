use clap::Command;
use clap_complete::{generate, Shell};
use std::io;

use crate::error::CliResult;

/// Generate shell completions for the given shell.
pub fn run(shell: Shell, cli: &mut Command) -> CliResult<()> {
    generate(shell, cli, "intr", &mut io::stdout());
    Ok(())
}
