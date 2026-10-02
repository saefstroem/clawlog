use super::claude;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown adapter '{0}'")]
    Unknown(String),
    #[error(transparent)]
    Claude(#[from] claude::Error),
}
