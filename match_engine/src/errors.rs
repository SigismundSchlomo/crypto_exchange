use thiserror::Error;

/// Engine-related errors
#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Order not found: {0}")]
    OrderNotFound(u64),

    #[error("Duplicate order ID: {0}")]
    DuplicateOrderId(u64),

    #[error("Processing error: {0}")]
    Processing(String),
}

/// Trait to simplify converting domain-specific errors into `anyhow::Error`
pub trait IntoAnyhow<T> {
    /// Convert a domain-specific error into an `anyhow::Error`
    fn into_anyhow(self) -> anyhow::Result<T>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> IntoAnyhow<T> for Result<T, E> {
    fn into_anyhow(self) -> anyhow::Result<T> {
        self.map_err(anyhow::Error::new)
    }
}
