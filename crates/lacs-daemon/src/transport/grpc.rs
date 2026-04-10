use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListenTarget {
    Unix(PathBuf),
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ListenTargetError {
    #[error("unsupported listen uri scheme: {0}")]
    UnsupportedScheme(String),

    #[error("invalid listen uri: {0}")]
    InvalidUri(String),

    #[error("existing path is not a unix socket: {0}")]
    ExistingPathNotSocket(String),

    #[error("io error: {0}")]
    Io(String),
}

impl ListenTarget {
    pub fn try_from_uri(uri: &str) -> Result<Self, ListenTargetError> {
        let Some(path) = uri.strip_prefix("unix://") else {
            return Err(ListenTargetError::UnsupportedScheme(uri.to_string()));
        };

        if path.is_empty() {
            return Err(ListenTargetError::InvalidUri(uri.to_string()));
        }

        if !Path::new(path).is_absolute() {
            return Err(ListenTargetError::InvalidUri(uri.to_string()));
        }

        Ok(Self::Unix(PathBuf::from(path)))
    }
}

pub fn bind_unix_listener(target: &ListenTarget) -> Result<UnixListener, ListenTargetError> {
    match target {
        ListenTarget::Unix(path) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|err| ListenTargetError::Io(err.to_string()))?;
            }

            if path.exists() {
                let file_type = fs::symlink_metadata(path)
                    .map_err(|err| ListenTargetError::Io(err.to_string()))?
                    .file_type();
                if !file_type.is_socket() {
                    return Err(ListenTargetError::ExistingPathNotSocket(
                        path.display().to_string(),
                    ));
                }

                fs::remove_file(path).map_err(|err| ListenTargetError::Io(err.to_string()))?;
            }

            UnixListener::bind(path).map_err(|err| ListenTargetError::Io(err.to_string()))
        }
    }
}
