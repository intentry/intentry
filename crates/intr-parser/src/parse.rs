use crate::types::{Eval, Frontmatter, ParseError, ParseResult, ParseWarning};

const MAX_FILE_SIZE: usize = 1024 * 1024; // 1 MB

/// Parse a `.prompt` file from raw bytes.
///
/// Determines the tier, splits frontmatter from body, extracts Handlebars
/// variables, and validates required fields.
///
/// # Errors
///
/// Returns [`ParseError`] if the file exceeds 1 MB, is not valid UTF-8, or
/// has malformed YAML frontmatter.
pub fn parse(bytes: &[u8]) -> Result<ParseResult, ParseError> {
    if bytes.len() > MAX_FILE_SIZE {
        return Err(ParseError::FileTooLarge { size: bytes.len() });
    }

    let src = std::str::from_utf8(bytes)
        .map_err(|e| ParseError::InvalidUtf8(e.to_string()))?;

    let (frontmatter_raw, body) = split_frontmatter(src);

    let (frontmatter, warnings) = match frontmatter_raw {
        Some(yaml) => parse_frontmatter(yaml)?,
        None => (None, vec![]),
    };

    let tier = detect_tier(&frontmatter);
    let variables = extract_variables(body);

    let mut all_warnings = warnings;
    lint_warnings(tier, &frontmatter, &mut all_warnings);

    Ok(ParseResult {
        tier,
        frontmatter,
        body: body.to_string(),
        variables,
        warnings: all_warnings,
    })
}

// ---------------------------------------------------------------------------
// Frontmatter splitting
// ---------------------------------------------------------------------------

/// Returns `(Some(yaml_str), body)` if a `---` fence is found, else
/// `(None, whole_source)`.
fn split_frontmatter(src: &str) -> (Option<&str>, &str) {
    let src = src.trim_start();

    if !src.starts_with("---") {
        return (None, src);
    }

    // Find the closing fence. Skip the opening `---` line.
    let after_open = &src[3..];
    // Consume optional trailing whitespace/newline on the opening fence line.
    let after_open = after_open.trim_start_matches([' ', '\t', '\r', '\n']);

    if let Some(close_pos) = find_closing_fence(after_open) {
        let yaml = &after_open[..close_pos];
        let rest = &after_open[close_pos + 3..];
        // Trim a single leading newline from the body.
        let body = rest.trim_start_matches(['\r', '\n']);
        (Some(yaml), body)
    } else {
        // No closing fence - treat entire file as body (Tier 1 fallback).
        (None, src)
    }
}

/// Find the position of the closing `---` fence within `haystack`.
/// Returns the byte offset of the `---` within `haystack`, or `None`.
fn find_closing_fence(haystack: &str) -> Option<usize> {
    for (i, _) in haystack.char_indices() {
        let rest = &haystack[i..];
        // A closing fence must be `---` at the start of a line.
        if (i == 0 || haystack.as_bytes().get(i - 1) == Some(&b'\n'))
            && rest.starts_with("---")
        {
            return Some(i);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Frontmatter parsing
// ---------------------------------------------------------------------------

fn parse_frontmatter(
    yaml: &str,
) -> Result<(Option<Frontmatter>, Vec<ParseWarning>), ParseError> {
    if yaml.trim().is_empty() {
        return Ok((None, vec![]));
    }

    let frontmatter: Frontmatter = serde_yaml::from_str(yaml)
        .map_err(|e| ParseError::InvalidFrontmatter(e.to_string()))?;

    let mut warnings = vec![];

    // Validate `version` is valid semver if present.
    if let Some(ref ver) = frontmatter.version {
        if semver_parse(ver).is_err() {
            return Err(ParseError::InvalidField {
                field: "version".to_string(),
                reason: format!("'{}' is not a valid semver string", ver),
            });
        }
    }

    // Validate `id` is kebab-case if present.
    if let Some(ref id) = frontmatter.id {
        if !is_valid_id(id) {
            return Err(ParseError::InvalidField {
                field: "id".to_string(),
                reason: format!(
                    "'{}' must be kebab-case, alphanumeric + hyphens, max 64 chars",
                    id
                ),
            });
        }
    }

    // Warn if temperature is out of [0, 2].
    if let Some(ref hints) = frontmatter.model {
        if let Some(temp) = hints.temperature {
            if !(0.0..=2.0).contains(&temp) {
                warnings.push(ParseWarning {
                    code: "temperature_out_of_range".to_string(),
                    message: format!("temperature {} is outside [0.0, 2.0]", temp),
                });
            }
        }
    }

    Ok((Some(frontmatter), warnings))
}

// ---------------------------------------------------------------------------
// Tier detection
// ---------------------------------------------------------------------------

fn detect_tier(frontmatter: &Option<Frontmatter>) -> u8 {
    let Some(fm) = frontmatter else {
        return 1;
    };

    // Tier 2 requires both `id` and `version`.
    if fm.id.is_none() || fm.version.is_none() {
        return 1;
    }

    // Tier 3 requires non-empty evals.
    if fm
        .evals
        .as_ref()
        .is_some_and(|e: &Vec<Eval>| !e.is_empty())
    {
        return 3;
    }

    2
}

// ---------------------------------------------------------------------------
// Variable extraction
// ---------------------------------------------------------------------------

/// Extract variable names from `{{variable}}` and `{{#if variable}}` markers.
///
/// Returns a sorted, de-duplicated list.
fn extract_variables(body: &str) -> Vec<String> {
    let mut vars = std::collections::BTreeSet::new();
    let mut chars = body.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'{') {
            chars.next(); // consume second `{`
            let mut name = String::new();

            // Skip Handlebars helpers like `#if`, `#each`, `/if`, etc.
            // We only capture bare variable names and the first arg of helpers.
            let mut skip_hash = false;
            if chars.peek() == Some(&'#') || chars.peek() == Some(&'/') {
                skip_hash = true;
                chars.next();
            }

            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
                if inner.is_whitespace() && !skip_hash {
                    break;
                }
                if inner == '}' {
                    break;
                }
                name.push(inner);
            }

            let name = name.trim().to_string();
            if !name.is_empty()
                && !skip_hash
                && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                // Strip any leading `@` (Handlebars special vars).
                let name = name.trim_start_matches('@').to_string();
                if !name.is_empty() {
                    vars.insert(name);
                }
            }
        }
    }

    vars.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Lint warnings
// ---------------------------------------------------------------------------

fn lint_warnings(
    tier: u8,
    frontmatter: &Option<Frontmatter>,
    warnings: &mut Vec<ParseWarning>,
) {
    if tier >= 2 {
        let fm = frontmatter.as_ref().expect("tier >= 2 implies frontmatter");

        if fm.description.is_none() {
            warnings.push(ParseWarning {
                code: "missing_description".to_string(),
                message:
                    "No `description` field. Add one to improve commons discoverability."
                        .to_string(),
            });
        }

        if fm.model.is_none() {
            warnings.push(ParseWarning {
                code: "missing_model_hints".to_string(),
                message:
                    "No `model` field. Specifying `model.preferred` improves reliability."
                        .to_string(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Minimal semver validation - just checks MAJOR.MINOR.PATCH pattern.
fn semver_parse(s: &str) -> Result<(), ()> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 3 {
        return Err(());
    }
    for part in &parts[..3] {
        if part.parse::<u64>().is_err() {
            return Err(());
        }
    }
    Ok(())
}

/// Validates an `id` field: kebab-case, alphanumeric + hyphens, 1–64 chars.
fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier1_plain_body() {
        let src = b"Summarize the following: {{input}}";
        let result = parse(src).unwrap();
        assert_eq!(result.tier, 1);
        assert!(result.frontmatter.is_none());
        assert!(result.variables.contains(&"input".to_string()));
    }

    #[test]
    fn tier2_with_frontmatter() {
        let src = b"---
id: greet
version: 1.0.0
description: Greet a user
model:
  preferred: [claude-sonnet-4-6]
  temperature: 0.3
input:
  schema:
    name: string
---
Hello, {{name}}!
";
        let result = parse(src).unwrap();
        assert_eq!(result.tier, 2);
        let fm = result.frontmatter.unwrap();
        assert_eq!(fm.id.as_deref(), Some("greet"));
        assert_eq!(fm.version.as_deref(), Some("1.0.0"));
        assert!(result.variables.contains(&"name".to_string()));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn tier3_with_evals() {
        let src = b"---
id: summarize
version: 2.0.0
description: One-sentence summary
evals:
  - description: Short text
    input:
      text: The sky is blue.
    expect:
      contains: blue
---
Summarize: {{text}}
";
        let result = parse(src).unwrap();
        assert_eq!(result.tier, 3);
        assert_eq!(result.frontmatter.unwrap().evals.unwrap().len(), 1);
    }

    #[test]
    fn invalid_version_is_error() {
        let src = b"---
id: bad-ver
version: not-semver
---
body
";
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ParseError::InvalidField { ref field, .. } if field == "version"));
    }

    #[test]
    fn invalid_id_is_error() {
        let src = b"---
id: -bad-start
version: 1.0.0
---
body
";
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ParseError::InvalidField { ref field, .. } if field == "id"));
    }

    #[test]
    fn file_too_large() {
        let huge = vec![b'x'; MAX_FILE_SIZE + 1];
        let err = parse(&huge).unwrap_err();
        assert!(matches!(err, ParseError::FileTooLarge { .. }));
    }

    #[test]
    fn missing_description_warns() {
        let src = b"---
id: no-desc
version: 1.0.0
---
body
";
        let result = parse(src).unwrap();
        assert!(result
            .warnings
            .iter()
            .any(|w| w.code == "missing_description"));
    }

    #[test]
    fn variables_deduped_sorted() {
        let src = b"{{b}} {{a}} {{b}} {{a}}";
        let result = parse(src).unwrap();
        assert_eq!(result.variables, vec!["a", "b"]);
    }
}
