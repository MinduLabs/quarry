//! Turns PostgreSQL failures into messages a person can act on, in the same three lines as the
//! MySQL driver: the server's own words, Quarry's explanation, and the exact code.

use std::error::Error as _;
use std::io::{self, ErrorKind};
use std::time::Duration;

use quarry_driver_api::DriverError;
use quarry_model::ErrorLocation;
use tokio_postgres::error::{DbError, ErrorPosition};

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

pub fn tls_setup(err: &rustls::Error) -> DriverError {
    DriverError::Connect(format!(
        "The secure connection (SSL/TLS) could not be set up.\nTry another SSL mode.\nCode: {err}"
    ))
}

pub fn connect_error(target: &Target, err: &tokio_postgres::Error) -> DriverError {
    if let Some(db) = err.as_db_error() {
        return server_error(target, db);
    }
    if let Some(io) = source_of::<io::Error>(err) {
        return io_error(target, io);
    }
    if let Some(tls) = source_of::<rustls::Error>(err) {
        return DriverError::Connect(message(
            target.could_not_connect(),
            "The secure connection (SSL/TLS) could not be established. Try another SSL mode, \
             or check the server certificate.",
            format!("TLS — {tls}"),
        ));
    }
    let text = err.to_string();
    let why = if text.contains("server does not support TLS") {
        "The server does not accept SSL/TLS. Set the SSL mode to Disable or Prefer."
    } else if text.contains("password missing") {
        "The server asks for a password. Enter it in the connection settings."
    } else if err.is_closed() {
        "The server closed the connection during the handshake. The port may not be a \
         PostgreSQL server, or the server may require a different SSL mode."
    } else {
        "Unexpected error while connecting."
    };
    DriverError::Connect(message(target.could_not_connect(), why, text))
}

fn source_of<T: std::error::Error + 'static>(err: &tokio_postgres::Error) -> Option<&T> {
    let mut source = err.source();
    while let Some(current) = source {
        if let Some(found) = current.downcast_ref::<T>() {
            return Some(found);
        }
        source = current.source();
    }
    None
}

fn io_error(target: &Target, err: &io::Error) -> DriverError {
    let code = match err.raw_os_error() {
        Some(os) => format!("{} (os error {os})", errno_name(err.kind())),
        None => err.to_string(),
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
        | ErrorKind::UnexpectedEof => {
            "The server closed the connection during the handshake. The port may not be a \
             PostgreSQL server, or the server may require a different SSL mode."
        }
        _ if is_dns_failure(err) => {
            "The host name can't be resolved. Check the spelling and your network connection."
        }
        _ => "A network error stopped the connection.",
    };
    DriverError::Connect(message(target.could_not_connect(), why, code))
}

/// `getaddrinfo` failures carry no errno, only text.
fn is_dns_failure(err: &io::Error) -> bool {
    let text = err.to_string();
    text.contains("lookup address")
        || text.contains("Name or service not known")
        || text.contains("nodename nor servname")
        || text.contains("Temporary failure in name resolution")
        || text.contains("failed to lookup")
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

fn server_error(target: &Target, db: &DbError) -> DriverError {
    let state = db.code().code();
    let headline = format!("{}: {}", db.severity(), db.message());
    let detail = format!("SQLSTATE {state}");
    match state {
        "28P01" => DriverError::Auth(message(
            headline,
            "The server rejected the user name or password.",
            detail,
        )),
        "28000" => DriverError::Auth(message(
            headline,
            "The server does not let this user in from your address with this method. A \
             database admin has to allow it in pg_hba.conf.",
            detail,
        )),
        "3D000" => DriverError::Connect(message(
            headline,
            "That database doesn't exist on the server. Check the Default database field, or \
             leave it empty to use the user's own.",
            detail,
        )),
        "53300" => DriverError::Connect(message(
            headline,
            "The server has too many connections open. Try again in a moment.",
            detail,
        )),
        "57P03" => DriverError::Connect(message(
            headline,
            "The server is starting up or shutting down. Try again in a moment.",
            detail,
        )),
        _ => DriverError::Connect(message(
            headline,
            &format!("The server at {} refused the connection.", target.address()),
            detail,
        )),
    }
}

/// An error from running `sql`.
pub fn query_error(err: &tokio_postgres::Error, sql: &str) -> DriverError {
    let Some(db) = err.as_db_error() else {
        if err.is_closed() || source_of::<io::Error>(err).is_some() {
            return DriverError::ConnectionLost(format!(
                "The connection to the server was lost: {err}."
            ));
        }
        return DriverError::Other(err.to_string().into());
    };
    let state = db.code().code();
    match state {
        "57014" => DriverError::Cancelled,
        "57P01" | "57P02" | "57P03" | "08000" | "08003" | "08006" => {
            DriverError::ConnectionLost(format!(
                "The server closed the connection: {} (SQLSTATE {state}).",
                db.message()
            ))
        }
        _ => {
            let mut message = format!("{} (SQLSTATE {state})", db.message());
            if let Some(detail) = db.detail() {
                message.push('\n');
                message.push_str(detail);
            }
            if let Some(hint) = db.hint() {
                message.push_str("\nHint: ");
                message.push_str(hint);
            }
            let location = match db.position() {
                Some(ErrorPosition::Original(position)) => location(sql, *position),
                _ => None,
            };
            DriverError::Query { message, location }
        }
    }
}

/// The line and the text from `position`, a 1-based character index into `sql`.
fn location(sql: &str, position: u32) -> Option<ErrorLocation> {
    let index = usize::try_from(position).ok()?.checked_sub(1)?;
    let (byte, _) = sql.char_indices().nth(index)?;
    let before = &sql[..byte];
    let line = u32::try_from(before.matches('\n').count() + 1).ok()?;
    let near = sql[byte..].lines().next().unwrap_or_default().to_owned();
    Some(ErrorLocation { line, near })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_characters_not_bytes() {
        let sql = "SELECT 'đ',\n  nope FROM t";
        let at = location(sql, 15).unwrap();
        assert_eq!(at.line, 2);
        assert_eq!(at.near, "nope FROM t");
        assert!(location(sql, 0).is_none());
        assert!(location(sql, 999).is_none());
    }
}
