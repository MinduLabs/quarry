//! What every driver says when a connection fails, in the shape the UI renders line by line:
//!
//! ```text
//! Couldn't connect to 127.0.0.1:5432          <- the headline (a driver may use the server's own)
//! Connection refused. Nothing is accepting…   <- why, and what to check
//! Code: ECONNREFUSED (os error 111)           <- the exact error, to search or report
//! ```
//!
//! A driver adds only what its server says: its error codes and how it reports them.

use std::io::{self, ErrorKind};
use std::time::Duration;

use crate::DriverError;

/// Where we tried to connect, as shown to the user.
#[derive(Debug)]
pub struct Target<'a> {
    pub host: &'a str,
    pub port: u16,
}

impl Target<'_> {
    pub fn address(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn could_not_connect(&self) -> String {
        format!("Couldn't connect to {}", self.address())
    }
}

/// The three lines of a failure.
pub fn message(headline: String, why: &str, code: String) -> String {
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

/// The TLS handshake failed; `detail` is the TLS library's text.
pub fn tls_failed(target: &Target, detail: &str) -> DriverError {
    DriverError::Connect(message(
        target.could_not_connect(),
        "The secure connection (SSL/TLS) could not be established. Try another SSL mode, \
         or check the server certificate.",
        format!("TLS — {detail}"),
    ))
}

/// The server hung up before signing in. `server` names what was expected ("a MySQL server").
pub fn closed_during_handshake(server: &str) -> String {
    format!(
        "The server closed the connection during the handshake. The port may not be {server}, \
         or the server may require a different SSL mode."
    )
}

/// A network failure while connecting, from its kind and text. `server` is as in
/// [`closed_during_handshake`].
pub fn network_error(
    headline: String,
    kind: ErrorKind,
    text: &str,
    code: String,
    server: &str,
) -> DriverError {
    let why = match kind {
        ErrorKind::ConnectionRefused => "Connection refused. Nothing is accepting connections on \
             that host and port — check that the server is running and the port is right. If the \
             database is only reachable through a tunnel or VPN, start that first."
            .to_owned(),
        ErrorKind::TimedOut => "No answer from the host. It may be unreachable, blocked by a \
             firewall, or the address may be wrong."
            .to_owned(),
        ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable => {
            "There is no route to the host. Check your network or VPN connection.".to_owned()
        }
        ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe
        | ErrorKind::UnexpectedEof => closed_during_handshake(server),
        _ if is_dns_failure(text) => {
            "The host name can't be resolved. Check the spelling and your network connection."
                .to_owned()
        }
        _ => "A network error stopped the connection.".to_owned(),
    };
    DriverError::Connect(message(headline, &why, code))
}

/// [`network_error`] for an `io::Error`, coded by its errno when it has one.
pub fn io_error(target: &Target, err: &io::Error, server: &str) -> DriverError {
    network_error(
        target.could_not_connect(),
        err.kind(),
        &err.to_string(),
        io_code(err),
        server,
    )
}

/// `ECONNREFUSED (os error 111)`, or the error's text when it carries no errno.
pub fn io_code(err: &io::Error) -> String {
    match err.raw_os_error() {
        Some(os) => format!("{} (os error {os})", errno_name(err.kind())),
        None => err.to_string(),
    }
}

/// `getaddrinfo` failures carry no errno, only text.
fn is_dns_failure(text: &str) -> bool {
    text.contains("lookup address")
        || text.contains("failed to lookup")
        || text.contains("Name or service not known")
        || text.contains("nodename nor servname")
        || text.contains("Temporary failure in name resolution")
}

fn errno_name(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::ConnectionRefused => "ECONNREFUSED",
        ErrorKind::TimedOut => "ETIMEDOUT",
        ErrorKind::HostUnreachable => "EHOSTUNREACH",
        ErrorKind::NetworkUnreachable => "ENETUNREACH",
        ErrorKind::ConnectionReset => "ECONNRESET",
        ErrorKind::ConnectionAborted => "ECONNABORTED",
        ErrorKind::BrokenPipe => "EPIPE",
        ErrorKind::PermissionDenied => "EACCES",
        _ => "I/O error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: Target = Target {
        host: "127.0.0.1",
        port: 3306,
    };

    fn text(err: DriverError) -> String {
        match err {
            DriverError::Connect(m) | DriverError::Auth(m) => m,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn refused_names_the_address_and_the_code() {
        let err = io::Error::from(ErrorKind::ConnectionRefused);
        let lines: Vec<_> = text(io_error(&TARGET, &err, "a MySQL server"))
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(lines[0], "Couldn't connect to 127.0.0.1:3306");
        assert!(lines[1].starts_with("Connection refused."));
        assert!(lines[2].starts_with("Code: "));
    }

    #[test]
    fn dns_failure_is_recognised() {
        let err =
            io::Error::other("failed to lookup address information: Name or service not known");
        assert!(text(io_error(&TARGET, &err, "a MySQL server")).contains("can't be resolved"));
    }

    #[test]
    fn a_closed_handshake_names_the_server_expected() {
        let err = io::Error::from(ErrorKind::UnexpectedEof);
        assert!(text(io_error(&TARGET, &err, "a SQL Server")).contains("may not be a SQL Server,"));
    }

    #[test]
    fn ipv6_addresses_are_bracketed() {
        let target = Target {
            host: "::1",
            port: 3306,
        };
        assert_eq!(target.address(), "[::1]:3306");
    }
}
