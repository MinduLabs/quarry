//! Turns PostgreSQL failures into messages a person can act on (see
//! `quarry_driver_api::diagnose`): the server's own words, Quarry's explanation, the exact code.

use std::error::Error as _;
use std::io;

use quarry_driver_api::DriverError;
pub use quarry_driver_api::diagnose::{Target, timed_out};
use quarry_driver_api::diagnose::{closed_during_handshake, io_error, message, tls_failed};
use quarry_model::ErrorLocation;
use tokio_postgres::error::{DbError, ErrorPosition};

/// What the server is, as `diagnose` names it in its explanations.
const SERVER: &str = "a PostgreSQL server";

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
        return io_error(target, io, SERVER);
    }
    if let Some(tls) = source_of::<rustls::Error>(err) {
        return tls_failed(target, &tls.to_string());
    }
    let text = err.to_string();
    let why = if text.contains("server does not support TLS") {
        "The server does not accept SSL/TLS. Set the SSL mode to Disable or Prefer.".to_owned()
    } else if text.contains("password missing") {
        "The server asks for a password. Enter it in the connection settings.".to_owned()
    } else if err.is_closed() {
        closed_during_handshake(SERVER)
    } else {
        "Unexpected error while connecting.".to_owned()
    };
    DriverError::Connect(message(target.could_not_connect(), &why, text))
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
