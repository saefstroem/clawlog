use std::io;

use crate::run::adapter;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Adapter(#[from] adapter::Error),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("invalid part file: {0}")]
    Json(#[from] serde_json::Error),
}
