use std::path::Path;

use intr_parser::parse;

use crate::{
    error::{CliError, CliResult},
    ui::output,
};

/// Validate a `.prompt` file and print its parse result.
pub fn run(path: &Path, json: bool) -> CliResult<()> {
    let bytes = std::fs::read(path)?;

    match parse(&bytes) {
        Ok(result) => {
            if json {
                output::print_json_ok(&result);
            } else {
                println!("File:  {}", path.display());
                println!("Tier:  {}", result.tier);
                println!("Body:  {} chars", result.body.len());
                if let Some(fm) = &result.frontmatter {
                    if let Some(id) = &fm.id {
                        println!("ID:    {id}");
                    }
                    if let Some(v) = &fm.version {
                        println!("Ver:   {v}");
                    }
                    if let Some(desc) = &fm.description {
                        println!("Desc:  {desc}");
                    }
                    if let Some(evals) = &fm.evals {
                        println!("Evals: {}", evals.len());
                    }
                }
                if !result.variables.is_empty() {
                    println!("Vars:  {}", result.variables.join(", "));
                }
                if !result.warnings.is_empty() {
                    for w in &result.warnings {
                        output::print_warn(&format!("[{}] {}", w.code, w.message));
                    }
                }

                output::print_success("valid .prompt file");
            }
        }
        Err(e) => {
            if json {
                output::print_json_error("parse_error", &e.to_string());
            } else {
                output::print_error(&format!("parse failed: {e}"));
            }
            return Err(CliError::Validation(e.to_string()));
        }
    }

    Ok(())
}
