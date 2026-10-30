//! Turns MySQL failures into messages a person can act on (see `quarry_driver_api::diagnose`).
//! A network failure's first line is what MySQL itself reports
//! (`Can't connect to MySQL server on '127.0.0.1:3306' (111)`), so it can be searched as is.

use std::io;

use mysql_async::{DriverError as MyDriverError, Error, IoError};
use quarry_driver_api::DriverError;
pub use quarry_driver_api::diagnose::{Target, timed_out};
use quarry_driver_api::diagnose::{
    closed_during_handshake, io_code, message, network_error, tls_failed,
};

/// What the server is, as `diagnose` names it in its explanations.
const SERVER: &str = "a MySQL server";

pub fn connect_error(target: &Target, err: Error) -> DriverError {
    match err {
        Error::Server(e) => server_error(target, e.code, &e.state, &e.message),
        Error::Io(IoError::Io(e)) => io_error(target, &e),
        Error::Io(IoError::Tls(e)) => tls_failed(target, &e.to_string()),
        Error::Driver(MyDriverError::UnknownAuthPlugin { name }) => DriverError::Connect(message(
            target.could_not_connect(),
            "The server uses a sign-in method Quarry doesn't support yet.",
            format!("unknown authentication plugin `{name}`"),
        )),
        Error::Driver(MyDriverError::ConnectionClosed) => DriverError::Connect(message(
            target.could_not_connect(),
            &closed_during_handshake(SERVER),
            "the server closed the connection".to_owned(),
        )),
        other => DriverError::Connect(message(
            target.could_not_connect(),
            "Unexpected error while connecting.",
            other.to_string(),
        )),
    }
}

fn io_error(target: &Target, err: &io::Error) -> DriverError {
    let headline = match err.raw_os_error() {
        Some(os) => format!(
            "Can't connect to MySQL server on '{}' ({os})",
            target.address()
        ),
        None => target.could_not_connect(),
    };
    network_error(headline, err.kind(), &err.to_string(), io_code(err), SERVER)
}

fn server_error(target: &Target, code: u16, state: &str, server_message: &str) -> DriverError {
    // The server's own line, as MySQL reports it.
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

    /// errno values differ per OS: `ECONNREFUSED` is 111 on Linux and 61 on macOS.
    #[cfg(target_os = "linux")]
    const ECONNREFUSED: i32 = 111;
    #[cfg(target_os = "macos")]
    const ECONNREFUSED: i32 = 61;

    #[test]
    fn os_error_number_is_kept() {
        let err = io::Error::from_raw_os_error(ECONNREFUSED);
        let msg = text(io_error(&TARGET, &err));
        assert_eq!(
            msg.lines().next(),
            Some(
                format!("Can't connect to MySQL server on '127.0.0.1:3306' ({ECONNREFUSED})")
                    .as_str()
            )
        );
        assert!(
            msg.contains(&format!("Code: ECONNREFUSED (os error {ECONNREFUSED})")),
            "{msg}"
        );
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
}
