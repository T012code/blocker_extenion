//! Errors, worded for people: these messages are shown in the app as they are.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The app was built without a Supabase project.
    NotConfigured,
    /// No connection to the server.
    Offline,
    /// Signed out, or the sign-in ended (expired or revoked).
    SignedOut,
    /// The code was wrong or expired.
    BadCode,
    /// Too many requests: the server asks to wait.
    TooMany,
    /// Something the person can fix (a malformed email, for example).
    Invalid(String),
    /// Anything else the server reported.
    Server { status: u16, message: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotConfigured => f.write_str("This copy of Smart Clip isn't connected to a server yet."),
            Error::Offline => f.write_str("Can't reach Smart Clip's server. Check your internet connection."),
            Error::SignedOut => f.write_str("You've been signed out. Sign in again to keep syncing."),
            Error::BadCode => f.write_str("That code is wrong or has expired. Check it, or ask for a new one."),
            Error::TooMany => f.write_str("Too many tries. Wait a minute, then try again."),
            Error::Invalid(m) => f.write_str(m),
            Error::Server { status, message } if message.is_empty() => {
                write!(f, "The server had a problem (error {status}). Try again in a moment.")
            }
            Error::Server { message, .. } => write!(f, "The server said: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        if e.is_decode() {
            Error::Server { status: 0, message: "an answer the app couldn't read".into() }
        } else {
            Error::Offline
        }
    }
}

impl Error {
    /// Network trouble that's worth retrying later.
    pub fn is_offline(&self) -> bool {
        matches!(self, Error::Offline) || matches!(self, Error::Server { status, .. } if *status >= 500 || *status == 0)
    }
}
