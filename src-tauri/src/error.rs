use std::fmt;

#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    Sidecar(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sidecar(m) => write!(f, "Sidecar: {m}"),
        }
    }
}

impl std::error::Error for AppError {}
