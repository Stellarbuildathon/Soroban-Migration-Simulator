use thiserror::Error;

/// Errors that can occur during migration report generation or serialization.
#[derive(Error, Debug)]
pub enum ReportError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
