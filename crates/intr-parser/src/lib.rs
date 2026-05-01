//! `intr-parser` - Parser for the `.prompt` file format.
//!
//! Parses Dotprompt-compatible `.prompt` files into a structured [`ParseResult`].
//! Supports all three tiers:
//!
//! - **Tier 1**: Plain Handlebars template with no frontmatter.
//! - **Tier 2**: YAML frontmatter with `id`, `version`, model hints, typed inputs.
//! - **Tier 3**: Tier 2 + `evals`, chains, reputation metadata.
//!
//! # Example
//!
//! ```rust
//! use intr_parser::parse;
//!
//! let src = r#"---
//! id: greet
//! version: 1.0.0
//! description: Greet a user by name
//! model:
//!   preferred: [claude-sonnet-4-6]
//!   temperature: 0.3
//! input:
//!   schema:
//!     name: string
//! ---
//! Hello, {{name}}!
//! "#;
//!
//! let result = parse(src.as_bytes()).unwrap();
//! assert_eq!(result.tier, 2);
//! assert!(result.variables.contains(&"name".to_string()));
//! ```

pub mod types;
mod parse;

pub use parse::parse;
pub use types::{
    Eval, EvalExpectation, Frontmatter, IntrEntryMeta, ModelHints, ParseError, ParseResult,
    ParseWarning, Picoschema,
};
