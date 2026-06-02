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

    #[error("project '{slug}' already exists")]
    AlreadyExists { slug: String },

    #[error("project '{slug}' not found")]
    NotFound { slug: String },

    #[error("'{name}' is not a valid project name")]
    InvalidName { name: String },
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
