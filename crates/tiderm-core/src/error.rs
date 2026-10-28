//! Error types for the core library.

/// A simple string-based error alias (the Tauri backend and native shell both surface
/// user-facing messages as `String` over their respective boundaries).
pub type Result<T> = std::result::Result<T, Error>;

/// A core error with a user-facing message.
#[derive(Debug, Clone)]
pub enum Error {
    /// A configuration / persistence error.
    Config(String),
    /// An SSH / SFTP error.
    Ssh(String),
    /// A local PTY / shell error.
    Pty(String),
    /// A Telnet protocol error.
    Telnet(String),
    /// A Serial port error.
    Serial(String),
    /// A raw TCP protocol error.
    Tcp(String),
    /// A vault / master-password error.
    Vault(String),
    /// A generic error.
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Config(m)
            | Error::Ssh(m)
            | Error::Pty(m)
            | Error::Telnet(m)
            | Error::Serial(m)
            | Error::Tcp(m)
            | Error::Vault(m)
            | Error::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Other(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::Other(s.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Other(e.to_string())
    }
}
