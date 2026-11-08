use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no git executable found in PATH; install git first")]
    GitMissing,

    #[error("`{path}` is not a directory")]
    NotADirectory { path: PathBuf },

    #[error("{path}: {message}")]
    Git { path: PathBuf, message: String },

    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot parse {path}: {message}")]
    Toml { path: PathBuf, message: String },

    #[error("no usable project roots: none of {tried} exists")]
    NoRoots { tried: String },

    #[error("{what}")]
    Unsupported { what: String },
}

pub type Result<T> = std::result::Result<T, Error>;
