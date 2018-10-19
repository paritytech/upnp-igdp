// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use httparse;
use roxmltree;
use std::{fmt, io, str};
use url;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// Failed to bind UDP socket.
    Bind,
    /// Action timed out.
    Timeout,
    /// Missing or invalid `Location` HTTP header.
    Location,
    /// Missing control URL in XML response.
    ControlUrl,
    /// Missing host and port information from URL.
    HostPort,
    /// Unexpected HTTP status code.
    StatusCode(Option<u16>),
    /// General I/O error.
    Io(io::Error),
    /// Error parsing HTTP response.
    Http(httparse::Error),
    /// Error parsing bytes as UTF-8.
    Utf8(str::Utf8Error),
    /// XML parsing error.
    Xml(roxmltree::Error),
    /// URL parsing error.
    Url(url::ParseError),
    /// Timer error.
    Timer,

    #[doc(hidden)]
    __Nonexhaustive
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Bind => f.write_str("error binding UDP socket"),
            Error::Timeout => f.write_str("timeout"),
            Error::Location => f.write_str("missing Location header"),
            Error::ControlUrl => f.write_str("missing control url"),
            Error::HostPort => f.write_str("missing host/port information in url"),
            Error::StatusCode(None) => f.write_str("missing http status code"),
            Error::StatusCode(Some(c)) => write!(f, "unexpected status code: {}", c),
            Error::Io(e) => write!(f, "i/o error: {}", e),
            Error::Http(e) => write!(f, "http parsing error: {}", e),
            Error::Utf8(e) => write!(f, "error parsing as utf-8: {}", e),
            Error::Xml(e) => write!(f, "xml parsing error: {}", e),
            Error::Url(e) => write!(f, "error parsing url: {}", e),
            Error::Timer => f.write_str("timer error"),
            Error::__Nonexhaustive => f.write_str("__Nonexhausive")
        }
    }
}

impl std::error::Error for Error {
    fn cause(&self) -> Option<&dyn std::error::Error> {
        match self {
            Error::Io(e) => Some(e),
            Error::Http(e) => Some(e),
            Error::Utf8(e) => Some(e),
            Error::Xml(e) => Some(e),
            Error::Url(e) => Some(e),
            _ => None
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<httparse::Error> for Error {
    fn from(e: httparse::Error) -> Self {
        Error::Http(e)
    }
}

impl From<str::Utf8Error> for Error {
    fn from(e: str::Utf8Error) -> Self {
        Error::Utf8(e)
    }
}

impl From<roxmltree::Error> for Error {
    fn from(e: roxmltree::Error) -> Self {
        Error::Xml(e)
    }
}

impl From<url::ParseError> for Error {
    fn from(e: url::ParseError) -> Self {
        Error::Url(e)
    }
}

