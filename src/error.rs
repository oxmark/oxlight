//! Error type of the crate.

/// Errors returned by oxlight.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A grammar or theme could not be parsed from JSON.
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// A grammar or theme registration is missing a required field or has an invalid shape.
    #[error("invalid {kind}: {message}")]
    Invalid {
        /// What was being loaded (`"language"` or `"theme"`).
        kind: &'static str,
        /// Human readable reason.
        message: String,
    },

    /// The requested language is not loaded.
    #[error("language `{0}` is not loaded")]
    UnknownLanguage(String),

    /// The requested theme is not loaded.
    #[error("theme `{0}` is not loaded")]
    UnknownTheme(String),

    /// The options are invalid (for example an empty set of themes).
    #[error("invalid options: {0}")]
    InvalidOptions(String),
}

/// Result alias used across the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
