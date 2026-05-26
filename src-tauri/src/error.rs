use std::fmt;

#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    Sidecar(String),
    Db(String),
    Io(String),
    NotFound(String),
    Validation(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sidecar(m) => write!(f, "Sidecar: {m}"),
            Self::Db(m) => write!(f, "Db: {m}"),
            Self::Io(m) => write!(f, "Io: {m}"),
            Self::NotFound(m) => write!(f, "NotFound: {m}"),
            Self::Validation(m) => write!(f, "Validation: {m}"),
        }
    }
}

impl std::error::Error for AppError {}
