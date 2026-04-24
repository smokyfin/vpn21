//! Crate-wide error type.

use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid profile: {0}")]
    InvalidProfile(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("dns error: {0}")]
    Dns(String),

    #[error("tunnel error: {0}")]
    Tunnel(String),

    #[error("arti error: {0}")]
    Arti(String),

    #[error("cancelled")]
    Cancelled,

    #[error("io: {0}")]
    Io(#[from] io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
