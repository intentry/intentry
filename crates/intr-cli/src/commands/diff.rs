use std::path::Path;

use intr_core::diff::{diff_content, format::text::render};

use crate::{
    error::{CliError, CliResult},
    ui::output,
};

/// Show semantic diff between two .prompt file versions or working copy vs HEAD.
///
/// Phase 1 (V1-003): diff between two files passed directly.
pub fn run(
    from_path: Option<&Path>,
    to_path: Option<&Path>,
    _show_all: bool,
    json: bool,
) -> CliResult<()> {
    match (from_path, to_path) {
        (Some(a), Some(b)) => {
            let from_content = std::fs::read_to_string(a)?;
            let to_content   = std::fs::read_to_string(b)?;

            let result = diff_content(&from_content, &to_content)
                .map_err(|e| CliError::Generic(e.to_string()))?;

            if json {
                output::print_json_ok(&result);
            } else {
                print!("{}", render(&result));
            }
        }
        _ => {
            // Working copy vs HEAD — stub for now (needs store wiring in Phase 3).
            eprintln!("note: working-copy diff requires store initialisation (Phase 3)");
            eprintln!("hint: use `intr diff <file-a> <file-b>` to compare two files directly");
        }
    }

    Ok(())
}
