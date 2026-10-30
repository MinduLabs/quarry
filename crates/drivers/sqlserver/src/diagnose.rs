//! Turns SQL Server failures into messages a person can act on (see
//! `quarry_driver_api::diagnose`): the server's own words, Quarry's explanation, the exact code.

use quarry_driver_api::DriverError;
pub use quarry_driver_api::diagnose::{Target, timed_out};
use quarry_driver_api::diagnose::{message, network_error, tls_failed};
use quarry_model::ErrorLocation;
use tiberius::error::Error;

/// What the server is, as `diagnose` names it in its explanations.
const SERVER: &str = "a SQL Server";

/// The TCP connection to the server failed.
pub fn io_error(target: &Target, err: &std::io::Error) -> DriverError {
    quarry_driver_api::diagnose::io_error(target, err, SERVER)
}

pub fn connect_error(target: &Target, err: &Error) -> DriverError {
    match err {
        // tiberius keeps the kind and the text of an io::Error, not its errno.
        Error::Io { kind, message } => network_error(
            target.could_not_connect(),
            *kind,
            message,
            message.clone(),
            SERVER,
        ),
        Error::Tls(text) => tls_failed(target, text),
        Error::Server(e) => {
            let headline = format!("Msg {}, Level {}: {}", e.code(), e.class(), e.message());
            let detail = format!("SQL Server error {}", e.code());
            match e.code() {
                18456 => DriverError::Auth(message(
                    headline,
                    "The server rejected the user name or password. A SQL Server login has to be \
                     enabled, and the server set to allow SQL Server authentication.",
                    detail,
                )),
                4060 => DriverError::Connect(message(
                    headline,
                    "That database doesn't exist on the server, or this login may not open it. \
                     Check the Default database field, or leave it empty.",
                    detail,
                )),
                _ => DriverError::Connect(message(
                    headline,
                    &format!("The server at {} refused the connection.", target.address()),
                    detail,
                )),
            }
        }
        other => DriverError::Connect(message(
            target.could_not_connect(),
            "Unexpected error while connecting.",
            other.to_string(),
        )),
    }
}

/// An error from running `sql`.
pub fn query_error(err: &Error, sql: &str) -> DriverError {
    match err {
        Error::Server(e) => {
            let message = format!(
                "{} (SQL Server error {}, level {})",
                e.message(),
                e.code(),
                e.class()
            );
            // Severity 20 and above ends the connection.
            if e.class() >= 20 {
                return DriverError::ConnectionLost(message);
            }
            DriverError::Query {
                message,
                location: location(sql, e.line()),
            }
        }
        Error::Io { message, .. } => DriverError::ConnectionLost(format!(
            "The connection to the server was lost: {message}. Reconnect to continue."
        )),
        other => DriverError::Other(other.to_string().into()),
    }
}

/// The `line` (from 1) of `sql` the server named, from its first non-blank character.
fn location(sql: &str, line: u32) -> Option<ErrorLocation> {
    let index = usize::try_from(line).ok()?.checked_sub(1)?;
    let text = sql.lines().nth(index)?;
    Some(ErrorLocation {
        line,
        near: text.trim_start().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_name_the_line() {
        let at = location("SELECT 1,\n   nope FROM t", 2).unwrap();
        assert_eq!(at.line, 2);
        assert_eq!(at.near, "nope FROM t");
        assert!(location("SELECT 1", 0).is_none());
        assert!(location("SELECT 1", 5).is_none());
    }
}
