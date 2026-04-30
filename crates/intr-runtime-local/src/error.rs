use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("parse error: {0}")]
    Parse(String),

    #[error("template rendering error: {0}")]
    Template(String),

    #[error("unknown model '{0}'")]
    UnknownModel(String),

    #[error("provider error: {0}")]
    Provider(String),
}
