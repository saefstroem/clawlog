use std::io;

use super::{adapter, browse};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Adapter(#[from] adapter::Error),
    #[error(transparent)]
    Browse(#[from] browse::Error),
    #[error("HOME is not set")]
    Home,
    #[error("{name} must be a whole number, got {value:?}")]
    Number { name: &'static str, value: String },
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("cannot encode entry: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no part number left for this conversation")]
    PartsExhausted,
    #[error("export target overlaps the log directory")]
    ExportIntoLogDir,
}
