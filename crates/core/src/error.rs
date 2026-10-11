use thiserror::Error;

/// Errors returned by [`crate::parser::parse`] when parsing or validating an OpenAPI document.
///
/// This is a concrete, matchable error type: callers can distinguish a malformed document from
/// one that is well-formed but declares an unsupported (or missing) `openapi` version without
/// parsing an error message string.
#[derive(Debug, Error)]
pub enum Error {
    /// The document content could not be parsed as either YAML or JSON.
    #[error("{0}")]
    Malformed(#[from] serde_json::Error),
    /// The document's declared (or missing) `openapi` field is not a supported version.
    #[error("{0}")]
    UnsupportedVersion(String),
}
