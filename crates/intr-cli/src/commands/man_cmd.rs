//! `intr man` - write man pages to a directory.
//!
//! Intended for use by packagers:
//! ```sh
//! intr man --output /usr/share/man/man1
//! ```
//!
//! Generates one `.1` man page per subcommand plus the root `intr.1`.

use std::path::Path;

use clap::Command;
use clap_mangen::Man;

use crate::error::{CliError, CliResult};

/// Write man pages for all commands to `out_dir`.
pub fn run(out_dir: &Path, cli: &mut Command) -> CliResult<()> {
    std::fs::create_dir_all(out_dir)?;

    // Root man page.
    write_page(cli, out_dir)?;

    // One man page per subcommand.
    for sub in cli.get_subcommands_mut() {
        write_page(sub, out_dir)?;
    }

    let count = std::fs::read_dir(out_dir)
        .map(|it| it.count())
        .unwrap_or(0);

    eprintln!("wrote {count} man page(s) to {}", out_dir.display());
    Ok(())
}

fn write_page(cmd: &mut Command, out_dir: &Path) -> CliResult<()> {
    let name = cmd.get_name().to_owned();
    let page_name = if name == "intr" {
        "intr.1".to_owned()
    } else {
        format!("intr-{name}.1")
    };
    let path = out_dir.join(&page_name);

    let man = Man::new(cmd.clone());
    let mut buf = Vec::new();
    man.render(&mut buf)
        .map_err(|e| CliError::Generic(format!("failed to render man page for {name}: {e}")))?;

    std::fs::write(&path, &buf)?;
    Ok(())
}
