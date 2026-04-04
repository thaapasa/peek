use thiserror::Error;

#[derive(Debug, Error)]
pub enum PeekError {
    #[error("unsupported file type: {0}")]
    UnsupportedType(String),

    #[error("image rendering failed: {0}")]
    ImageRender(String),

    #[error("no syntax definition found for: {0}")]
    NoSyntax(String),
}
