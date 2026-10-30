//! `quarry-driver-sqlserver`: serves SQL Server sessions to the engine (`quarry-driver-proto`).

use std::process::ExitCode;
use std::sync::Arc;

use quarry_driver_sqlserver::SqlServerDriver;

fn main() -> ExitCode {
    // tiberius builds rustls with aws-lc-rs too; ring is the provider Quarry uses everywhere.
    let _ = rustls::crypto::ring::default_provider().install_default();
    quarry_driver_proto::driver_main(vec![Arc::new(SqlServerDriver)])
}
