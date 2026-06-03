use std::path::PathBuf;

/// Errors returned by the `mustr` library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid TOML at {path}: {source}")]
    TomlRead {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to serialize TOML for {path}: {source}")]
    TomlWrite {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },

    #[error("{kind} '{slug}' already exists")]
    AlreadyExists { kind: &'static str, slug: String },

    #[error("{kind} '{slug}' not found")]
    NotFound { kind: &'static str, slug: String },

    #[error("'{name}' is not a valid name")]
    InvalidName { name: String },

    #[error("'{slug}' is a reserved folder and cannot be modified")]
    Reserved { slug: String },
}

impl Error {
    /// Wraps an [`std::io::Error`] with the path it occurred at.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}

/// Convenience alias for results in this crate.
pub type Result<T> = std::result::Result<T, Error>;
