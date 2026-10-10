#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("missing_or_invalid_input: {0}")]
    Input(String),
    #[error("state_capacity_exceeded")]
    Capacity,
    #[error("out_of_order_event")]
    OutOfOrder,
    #[error("state_error: {0}")]
    State(String),
}
pub type EngineResult<T> = std::result::Result<T, EngineError>;
