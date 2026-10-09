use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Usage,
    Input,
    Unavailable,
    Auth,
    Config,
    Io,
    Budget,
    Uncertain,
    Interrupted,
}

#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    message: String,
    io_kind: Option<std::io::ErrorKind>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            io_kind: None,
        }
    }

    pub fn io(context: &str, error: std::io::Error) -> Self {
        Self {
            kind: ErrorKind::Io,
            message: format!("{context}: {error}"),
            io_kind: Some(error.kind()),
        }
    }

    pub fn is_broken_pipe(&self) -> bool {
        self.io_kind == Some(std::io::ErrorKind::BrokenPipe)
    }

    pub fn exit_code(&self) -> u8 {
        match self.kind {
            ErrorKind::Usage => 64,
            ErrorKind::Input => 65,
            ErrorKind::Unavailable => 69,
            ErrorKind::Io => 74,
            ErrorKind::Auth => 77,
            ErrorKind::Config => 78,
            ErrorKind::Budget => 4,
            ErrorKind::Uncertain => 3,
            ErrorKind::Interrupted => 130,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}
