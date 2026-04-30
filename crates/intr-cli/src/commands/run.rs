//! `intr run` — Execute a prompt against a model provider.

use intr_core::store::VersionStore;
use intr_runtime_local::{run as execute, RunInput};
use serde_json::Value;

use crate::{
    error::{CliError, CliResult},
    store::SpaceCtx,
    ui::output,
};

// ---------------------------------------------------------------------------
// Public command entry point
// ---------------------------------------------------------------------------

pub async fn run(
    prompt_ref: &str,
    input: Option<&str>,
    model: Option<&str>,
    _stream: bool, // reserved — streaming API is Phase 5+
    json: bool,
) -> CliResult<()> {
    let content = resolve_prompt_content(prompt_ref).await?;
    let variables = parse_input(input)?;

    let run_input = RunInput {
        prompt_content: content,
        variables,
        model_override: model.map(str::to_owned),
    };

    let out = execute(run_input)
        .await
        .map_err(|e| CliError::Generic(e.to_string()))?;

    if json {
        output::print_json_ok(&serde_json::json!({
            "text": out.text,
            "model": out.model_used,
            "tokens_in": out.tokens_in,
            "tokens_out": out.tokens_out,
            "latency_ms": out.latency_ms,
            "cost_usd": out.cost_usd,
        }));
    } else {
        println!("{}", out.text);
        let cost_str = out
            .cost_usd
            .map(|c| format!(" · ${c:.6}"))
            .unwrap_or_default();
        output::print_info(&format!(
            "  {} · {} in · {} out · {}ms{cost_str}",
            out.model_used, out.tokens_in, out.tokens_out, out.latency_ms,
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Prompt resolution (shared with `eval.rs`)
// ---------------------------------------------------------------------------

/// Load raw `.prompt` source from a file path or a local store slug.
///
/// Detection rules:
/// - Contains `/` or `\`, ends with `.prompt`, or starts with `.`/`~`/`/` → file path
/// - Otherwise → local slug lookup (`<slug>.prompt` in cwd, then store)
pub(super) async fn resolve_prompt_content(prompt_ref: &str) -> CliResult<String> {
    let is_path = prompt_ref.contains('/')
        || prompt_ref.contains('\\')
        || prompt_ref.ends_with(".prompt")
        || prompt_ref.starts_with('.')
        || prompt_ref.starts_with('~')
        || std::path::Path::new(prompt_ref).is_absolute();

    if is_path {
        let path = if prompt_ref.starts_with('~') {
            let home = dirs::home_dir()
                .ok_or_else(|| CliError::Generic("cannot resolve home directory".into()))?;
            home.join(prompt_ref.trim_start_matches("~/"))
        } else {
            std::path::PathBuf::from(prompt_ref)
        };
        return std::fs::read_to_string(&path)
            .map_err(|e| CliError::Generic(format!("cannot read {}: {e}", path.display())));
    }

    // Strip optional @version suffix (future feature).
    let slug = prompt_ref
        .rfind('@')
        .map(|i| &prompt_ref[..i])
        .unwrap_or(prompt_ref);

    // Quick path: `<slug>.prompt` in the working directory.
    let local_filename = format!("{slug}.prompt");
    let local_file = std::path::Path::new(&local_filename);
    if local_file.exists() {
        return Ok(std::fs::read_to_string(local_file)?);
    }

    // Fall back to local `.intr/` store.
    let ctx = SpaceCtx::open().await?;
    let prompt = ctx
        .store
        .get_prompt_by_slug(&ctx.space.id, slug)
        .await
        .map_err(|_| {
            CliError::Generic(format!(
                "prompt '{slug}' not found — run `intr list` to see available prompts"
            ))
        })?;
    let commit = ctx
        .store
        .get_commit(&prompt.head_commit_id)
        .await
        .map_err(|e| CliError::Generic(e.to_string()))?;
    let bytes = ctx
        .store
        .get_blob(&commit.content_hash)
        .await
        .map_err(|e| CliError::Generic(e.to_string()))?;
    String::from_utf8(bytes)
        .map_err(|_| CliError::Generic(format!("prompt '{slug}' content is not valid UTF-8")))
}

// ---------------------------------------------------------------------------
// Input parsing
// ---------------------------------------------------------------------------

/// Parse the `--input` CLI value into a JSON `Value::Object`.
///
/// Accepted forms:
/// - `'{"key": "value"}'`  — JSON object (passed through)
/// - `'plain text'`        — wrapped as `{"input": "plain text"}`
/// - None                  — empty object `{}`
pub(super) fn parse_input(input: Option<&str>) -> CliResult<Value> {
    match input {
        None => Ok(Value::Object(Default::default())),
        Some(s) => {
            let s = s.trim();
            if s.starts_with('{') {
                serde_json::from_str(s).map_err(|e| {
                    CliError::Validation(format!(
                        "invalid JSON input: {e}\n  hint: use --input '{{\"key\": \"value\"}}'"
                    ))
                })
            } else {
                // Single-variable shorthand: `{"input": "<value>"}`
                Ok(serde_json::json!({ "input": s }))
            }
        }
    }
}

