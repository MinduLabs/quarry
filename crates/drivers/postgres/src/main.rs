//! `quarry-driver-postgres`: serves PostgreSQL sessions to the engine (`quarry-driver-proto`).

use std::process::ExitCode;
use std::sync::Arc;

use quarry_driver_postgres::PostgresDriver;

fn main() -> ExitCode {
    // A workspace build gives rustls both ring and aws-lc-rs; the driver's own TLS names ring,
    // and this keeps anything else that asks for the default on it too.
    let _ = rustls::crypto::ring::default_provider().install_default();
    quarry_driver_proto::driver_main(vec![Arc::new(PostgresDriver)])
}
