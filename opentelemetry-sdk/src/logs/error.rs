use crate::ExportError;

use std::{sync::PoisonError, time::Duration};
use thiserror::Error;

/// Describe the result of operations in log SDK.
pub type LogResult<T> = Result<T, LogError>;

#[derive(Error, Debug)]
#[non_exhaustive]
/// Errors returned by the log SDK.
pub enum LogError {
    /// Export failed with the error returned by the exporter.
    #[error("Exporter {0} encountered the following errors: {name}", name = .0.exporter_name())]
    ExportFailed(Box<dyn ExportError>),

    /// Export failed to finish after certain period and processor stopped the export.
    #[error("Exporter timed out after {} seconds", .0.as_secs())]
    ExportTimedOut(Duration),

    /// Processor is already shutdown
    #[error("{0} already shutdown")]
    AlreadyShutdown(String),

    /// Mutex lock poisoning
    #[error("mutex lock poisioning for {0}")]
    MutexPoisoned(String),

    /// Other errors propagated from log SDK that weren't covered above.
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl<T> From<T> for LogError
where
    T: ExportError,
{
    fn from(err: T) -> Self {
        LogError::ExportFailed(Box::new(err))
    }
}

impl From<String> for LogError {
    fn from(err_msg: String) -> Self {
        LogError::Other(Box::new(Custom(err_msg)))
    }
}

impl From<&'static str> for LogError {
    fn from(err_msg: &'static str) -> Self {
        LogError::Other(Box::new(Custom(err_msg.into())))
    }
}

impl<T> From<PoisonError<T>> for LogError {
    fn from(err: PoisonError<T>) -> Self {
        LogError::Other(err.to_string().into())
    }
}
/// Wrap type for string
#[derive(Error, Debug)]
#[error("{0}")]
struct Custom(String);
