//! Turns SQL Server failures into messages a person can act on, in the same three lines as the
//! other drivers: the server's own words, Quarry's explanation, and the exact code.

use std::io::ErrorKind;
use std::time::Duration;

use quarry_driver_api::DriverError;
use quarry_model::ErrorLocation;
use tiberius::error::Error;

/// Where we tried to connect, as shown to the user.
pub struct Target<'a> {
    pub host: &'a str,
    pub port: u16,
}

impl Target<'_> {
    fn address(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    fn could_not_connect(&self) -> String {
        format!("Couldn't connect to {}", self.address())
    }
}

fn message(headline: String, why: &str, code: String) -> String {
    format!("{headline}\n{why}\nCode: {code}")
}

pub fn timed_out(target: &Target, after: Duration) -> DriverError {
    DriverError::Connect(message(
        target.could_not_connect(),
        &format!(
            "No answer within {} s. The host may be unreachable, blocked by a firewall, \
             or the address may be wrong.",
            after.as_secs()
        ),
        "ETIMEDOUT (connection attempt timed out)".to_owned(),
    ))
}

pub fn io_error(target: &Target, kind: ErrorKind, text: &str) -> DriverError {
    let why = match kind {
        ErrorKind::ConnectionRefused => {
            "Connection refused. Nothing is accepting connections on that host and port — check \
             that the server is running and the port is right. If the database is only reachable \
             through a tunnel or VPN, start that first."
        }
        ErrorKind::TimedOut => {
            "No answer from the host. It may be unreachable, blocked by a firewall, or the \
             address may be wrong."
        }
        ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable => {
            "There is no route to the host. Check your network or VPN connection."
        }
        ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe
        | ErrorKind::UnexpectedEof => {
            "The server closed the connection during the handshake. The port may not be a SQL \
             Server, or the server may require a different SSL mode."
        }
        _ if text.contains("failed to lookup")
            || text.contains("Name or service not known")
            || text.contains("nodename nor servname") =>
        {
            "The host name can't be resolved. Check the spelling and your network connection."
        }
        _ => "A network error stopped the connection.",
    };
    DriverError::Connect(message(target.could_not_connect(), why, text.to_owned()))
}

pub fn connect_error(target: &Target, err: &Error) -> DriverError {
    match err {
        Error::Io { kind, message } => io_error(target, *kind, message),
        Error::Tls(text) => DriverError::Connect(message(
            target.could_not_connect(),
            "The secure connection (SSL/TLS) could not be established. Try another SSL mode, \
             or check the server certificate.",
            format!("TLS — {text}"),
        )),
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
