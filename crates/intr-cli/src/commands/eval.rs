//! `intr eval` - Run embedded eval cases for a prompt and check assertions.

use intr_parser::{parse, EvalExpectation};
use intr_runtime_local::{run as execute, RunInput};
use owo_colors::OwoColorize;

use crate::{
    error::{CliError, CliResult},
    ui::output,
};

use super::run::resolve_prompt_content;

// ---------------------------------------------------------------------------
// Public command entry point
// ---------------------------------------------------------------------------

pub async fn run(prompt_ref: &str, name_filter: Option<&str>, json: bool) -> CliResult<()> {
    let content = resolve_prompt_content(prompt_ref).await?;

    let parsed =
        parse(content.as_bytes()).map_err(|e| CliError::Validation(format!("parse error: {e}")))?;

    let evals = parsed
        .frontmatter
        .as_ref()
        .and_then(|fm| fm.evals.as_deref())
        .filter(|e| !e.is_empty())
        .ok_or_else(|| {
            CliError::Generic(format!(
                "'{prompt_ref}' has no evals\n  hint: add an `evals:` block to your .prompt frontmatter"
            ))
        })?;

    let filtered: Vec<_> = evals
        .iter()
        .filter(|e| {
            name_filter
                .map(|n| e.description.as_deref().unwrap_or("").contains(n))
                .unwrap_or(true)
        })
        .collect();

    if filtered.is_empty() {
        return Err(CliError::Generic(format!(
            "no evals match filter '{}'",
            name_filter.unwrap_or("")
        )));
    }

    let total = filtered.len();
    let mut passed = 0usize;

    // Collect results before printing so we can do a clean summary.
    struct EvalResult {
        description: String,
        output: String,
        model: String,
        assertions: Vec<(String, bool)>,
        all_pass: bool,
    }

    let mut results: Vec<EvalResult> = Vec::new();

    for eval_case in &filtered {
        let description = eval_case
            .description
            .clone()
            .unwrap_or_else(|| "(unnamed)".to_owned());

        let out = execute(RunInput {
            prompt_content: content.clone(),
            variables: eval_case.input.clone(),
            model_override: None,
        })
        .await
        .map_err(|e| CliError::Generic(e.to_string()))?;

        let assertions = check_assertions(&out.text, eval_case.expect.as_ref());
        let all_pass = assertions.iter().all(|(_, ok)| *ok);

        if all_pass {
            passed += 1;
        }

        results.push(EvalResult {
            description,
            output: out.text,
            model: out.model_used,
            assertions,
            all_pass,
        });
    }

    if json {
        output::print_json_ok(&serde_json::json!({
            "total": total,
            "passed": passed,
            "failed": total - passed,
            "results": results.iter().map(|r| serde_json::json!({
                "description": r.description,
                "passed": r.all_pass,
                "output": r.output,
                "model": r.model,
                "assertions": r.assertions.iter().map(|(a, ok)| serde_json::json!({
                    "assertion": a, "passed": ok
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }));
    } else {
        for r in &results {
            if r.all_pass {
                println!("{} {}", "✓".green().bold(), r.description);
            } else {
                println!("{} {}", "✗".red().bold(), r.description);
                println!("  output: {}", r.output.trim());
                for (assertion, ok) in &r.assertions {
                    if !ok {
                        println!("  {} {assertion}", "✗".red());
                    }
                }
            }
        }
        println!();
        if passed == total {
            output::print_success(&format!("{passed}/{total} passed"));
        } else {
            output::print_error(&format!(
                "{passed}/{total} passed ({} failed)",
                total - passed
            ));
        }
    }

    if passed < total {
        return Err(CliError::Generic(format!("{} eval(s) failed", total - passed)));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Assertion checking
// ---------------------------------------------------------------------------

/// Evaluate all assertions in an [`EvalExpectation`].
/// Returns `Vec<(human_description, passed)>`.
fn check_assertions(
    output_text: &str,
    expect: Option<&EvalExpectation>,
) -> Vec<(String, bool)> {
    let Some(e) = expect else {
        return vec![];
    };

    let mut results = Vec::new();

    if let Some(s) = &e.contains {
        results.push((
            format!("output contains {s:?}"),
            output_text.contains(s.as_str()),
        ));
    }
    if let Some(s) = &e.not_contains {
        results.push((
            format!("output does not contain {s:?}"),
            !output_text.contains(s.as_str()),
        ));
    }
    if let Some(s) = &e.equals {
        results.push((
            format!("output equals {s:?}"),
            output_text.trim() == s.trim(),
        ));
    }

    results
}

