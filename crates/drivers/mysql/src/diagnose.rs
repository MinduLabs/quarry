//! Turns connection failures into messages a person can act on.
//!
//! Every message has the same shape, which the UI renders line by line:
//!
//! ```text
//! Can't connect to MySQL server on '127.0.0.1:3306' (111)   <- the server's own wording and code
//! Connection refused. Nothing is accepting…                 <- why, and what to check
//! Code: ECONNREFUSED (os error 111)                         <- the exact error, to search or report
//! ```
//!
//! The first line is what MySQL clients (mysql, TablePlus, DBeaver) print, so it can be searched
//! as is; the second line is Quarry's explanation.

use std::io::{self, ErrorKind};
use std::time::Duration;

use mysql_async::{DriverError as MyDriverError, Error, IoError};
use quarry_driver_api::DriverError;

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

pub fn connect_error(target: &Target, err: Error) -> DriverError {
    match err {
        Error::Server(e) => server_error(target, e.code, &e.state, &e.message),
        Error::Io(IoError::Io(e)) => io_error(target, &e),
        Error::Io(IoError::Tls(e)) => DriverError::Connect(message(
            target.could_not_connect(),
            "The secure connection (SSL/TLS) could not be established. Try another SSL mode, \
             or check the server certificate.",
            format!("TLS — {e}"),
        )),
        Error::Driver(MyDriverError::UnknownAuthPlugin { name }) => DriverError::Connect(message(
            target.could_not_connect(),
            "The server uses a sign-in method Quarry doesn't support yet.",
            format!("unknown authentication plugin `{name}`"),
        )),
        Error::Driver(MyDriverError::ConnectionClosed) => DriverError::Connect(message(
            target.could_not_connect(),
            CLOSED_HINT,
            "the server closed the connection".to_owned(),
        )),
        other => DriverError::Connect(message(
            target.could_not_connect(),
            "Unexpected error while connecting.",
            other.to_string(),
        )),
    }
}

const CLOSED_HINT: &str = "The server closed the connection during the handshake. The port may not be a \
     MySQL server, or the server may require a different SSL mode.";

fn io_error(target: &Target, err: &io::Error) -> DriverError {
    let code = match err.raw_os_error() {
        Some(os) => format!("{} (os error {os})", errno_name(err.kind())),
        None => err.to_string(),
    };
    let headline = match err.raw_os_error() {
        Some(os) => format!(
            "Can't connect to MySQL server on '{}' ({os})",
            target.address()
        ),
        None => target.could_not_connect(),
    };
    let why = match err.kind() {
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
        | ErrorKind::UnexpectedEof => CLOSED_HINT,
        _ if is_dns_failure(err) => {
            "The host name can't be resolved. Check the spelling and your network connection."
        }
        _ => "A network error stopped the connection.",
    };
    DriverError::Connect(message(headline, why, code))
}

/// `getaddrinfo` failures carry no errno, only text.
fn is_dns_failure(err: &io::Error) -> bool {
    let text = err.to_string();
    text.contains("lookup address")
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

fn server_error(target: &Target, code: u16, state: &str, server_message: &str) -> DriverError {
    // The server's own line, as the `mysql` client prints it.
    let headline = format!("ERROR {code} ({state}): {server_message}");
    let detail = format!("MySQL error {code}, SQLSTATE {state}");
    match code {
        1045 => DriverError::Auth(message(
            headline,
            "The server rejected the user name or password.",
            detail,
        )),
        1044 | 1142 => DriverError::Auth(message(
            headline,
            "The user exists, but isn't allowed to use that database.",
            detail,
        )),
        1130 => DriverError::Connect(message(
            headline,
            "The server doesn't accept this user from your address. A database admin has to \
             allow your host for that user.",
            detail,
        )),
        1049 => DriverError::Connect(message(
            headline,
            "That database doesn't exist on the server. Check the Default database field, or \
             leave it empty.",
            detail,
        )),
        1040 | 1203 => DriverError::Connect(message(
            headline,
            "The server has too many connections open. Try again in a moment.",
            detail,
        )),
        _ => DriverError::Connect(message(
            headline,
            &format!("The server at {} refused the connection.", target.address()),
            detail,
        )),
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
        let lines: Vec<_> = text(io_error(&TARGET, &err))
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(lines[0], "Couldn't connect to 127.0.0.1:3306");
        assert!(lines[1].starts_with("Connection refused."));
        assert!(lines[2].starts_with("Code: "));
    }

    #[test]
    fn os_error_number_is_kept() {
        let err = io::Error::from_raw_os_error(111);
        let msg = text(io_error(&TARGET, &err));
        assert_eq!(
            msg.lines().next(),
            Some("Can't connect to MySQL server on '127.0.0.1:3306' (111)")
        );
        assert!(msg.contains("Code: ECONNREFUSED (os error 111)"), "{msg}");
    }

    #[test]
    fn dns_failure_is_recognised() {
        let err =
            io::Error::other("failed to lookup address information: Name or service not known");
        assert!(text(io_error(&TARGET, &err)).contains("can't be resolved"));
    }

    #[test]
    fn access_denied_is_an_auth_error_with_the_server_text() {
        let err = server_error(&TARGET, 1045, "28000", "Access denied for user 'root'");
        let DriverError::Auth(msg) = err else {
            panic!("expected Auth")
        };
        assert!(msg.starts_with("ERROR 1045 (28000): Access denied for user 'root'"));
        assert!(msg.ends_with("Code: MySQL error 1045, SQLSTATE 28000"));
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
