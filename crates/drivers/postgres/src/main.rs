//! `quarry-driver-postgres`: serves PostgreSQL sessions to the engine (`quarry-driver-proto`).

use std::process::ExitCode;
use std::sync::Arc;

use quarry_driver_postgres::PostgresDriver;

fn main() -> ExitCode {
    quarry_driver_proto::driver_main(vec![Arc::new(PostgresDriver)])
}
