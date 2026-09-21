use sigy_core::{budget::BudgetError, money::MoneyError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("library already owned by another process")]
    LibraryBusy,
    #[error("control protocol failed: {0}")]
    Protocol(&'static str),
    #[error("control message is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("service request timed out; a submitted change may have completed")]
    Timeout,
    #[error("service is shutting down")]
    ServiceStopped,
    #[error("service rejected the request: {0}")]
    Remote(String),
    #[error("catalog operation failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Budget(#[from] BudgetError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error("invalid {0}")]
    InvalidInput(&'static str),
    #[error("catalog schema {found} is newer than supported schema {supported}")]
    FutureSchema { found: i64, supported: i64 },
    #[error("the database is not a recognized Sigy catalog")]
    ForeignCatalog,
    #[error("catalog integrity check failed; preserve the library for recovery")]
    CatalogIntegrity,
    #[error("record not found")]
    NotFound,
    #[error("idempotency key was reused with different request parameters")]
    IdempotencyConflict,
    #[error("request state does not permit this operation")]
    RequestState,
    #[error("ledger integrity check failed")]
    LedgerIntegrity,
    #[error("capture journal integrity check failed")]
    CaptureIntegrity,
    #[error("capture revision or worker generation is stale; reread current state")]
    StaleCapture,
    #[error("capture admission limit reached")]
    CaptureCapacity,
    #[error(transparent)]
    CaptureTransition(#[from] sigy_core::capture::InvalidTransition),
    #[error("system clock cannot be represented: {0}")]
    Clock(#[from] std::time::SystemTimeError),
}

pub type Result<T> = std::result::Result<T, Error>;
