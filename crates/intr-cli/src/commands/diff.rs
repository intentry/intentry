use std::path::Path;

use intr_core::diff::{diff_content, format::text::render};
use intr_runtime_local::{run as execute, RunInput};
use similar::{ChangeTag, TextDiff};

use crate::{
    error::{CliError, CliResult},
    ui::output,
};

use super::run::parse_input;

/// Show semantic diff between two .prompt file versions or working copy vs HEAD.
///
/// With `--output-diff`, also runs both files and diffs their model outputs.
pub async fn run(
    from_path: Option<&Path>,
    to_path: Option<&Path>,
    _show_all: bool,
    output_diff: bool,
    input: Option<&str>,
    model: Option<&str>,
    json: bool,
) -> CliResult<()> {
    match (from_path, to_path) {
        (Some(a), Some(b)) => {
            let from_content = std::fs::read_to_string(a)?;
            let to_content   = std::fs::read_to_string(b)?;

            let result = diff_content(&from_content, &to_content)
                .map_err(|e| CliError::Generic(e.to_string()))?;

            if output_diff {
                // Run both files and show a diff of their outputs.
                let variables = parse_input(input)?;

                let from_out = execute(RunInput {
                    prompt_content: from_content,
                    variables: variables.clone(),
                    model_override: model.map(str::to_owned),
                })
                .await
                .map_err(|e| CliError::Generic(format!("running {}: {e}", a.display())))?;

                let to_out = execute(RunInput {
                    prompt_content: to_content,
                    variables,
                    model_override: model.map(str::to_owned),
                })
                .await
                .map_err(|e| CliError::Generic(format!("running {}: {e}", b.display())))?;

                if json {
                    output::print_json_ok(&serde_json::json!({
                        "from": { "file": a.display().to_string(), "output": from_out.text },
                        "to":   { "file": b.display().to_string(), "output": to_out.text },
                        "prompt_diff": result,
                    }));
                } else {
                    // Show the structural prompt diff first.
                    print!("{}", render(&result));

                    // Then show the output diff.
                    println!("\n--- output: {}", a.display());
                    println!("+++ output: {}", b.display());
                    let text_diff =
                        TextDiff::from_lines(from_out.text.as_str(), to_out.text.as_str());
                    for change in text_diff.iter_all_changes() {
                        let prefix = match change.tag() {
                            ChangeTag::Delete => "-",
                            ChangeTag::Insert => "+",
                            ChangeTag::Equal  => " ",
                        };
                        print!("{prefix}{}", change.value());
                    }
                }
            } else if json {
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

