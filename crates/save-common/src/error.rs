use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn validation(msg: impl Into<String>) -> Self {
        Error::Validation(msg.into())
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Error::Config(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = Error::validation("invalid bucket name");
        assert!(matches!(err, Error::Validation(_)));
        assert_eq!(err.to_string(), "Validation error: invalid bucket name");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn test_result_type() {
        let ok_result: Result<i32> = Ok(42);
        assert!(ok_result.is_ok());

        let err_result: Result<i32> = Err(Error::validation("test"));
        assert!(err_result.is_err());
    }
}
