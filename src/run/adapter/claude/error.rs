use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0} of settings.json must be a JSON object")]
    NotObject(&'static str),
    #[error("hooks.{0} of settings.json must be a JSON array")]
    NotArray(String),
}
