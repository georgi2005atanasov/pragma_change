#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ValidationError(pub String);
pub type Result<T> = std::result::Result<T, ValidationError>;
pub(crate) fn require(ok: bool, msg: impl Into<String>) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(ValidationError(msg.into()))
    }
}
