//! `quarry-driver-mysql`: serves MySQL and MariaDB sessions to the engine (`quarry-driver-proto`).

use std::process::ExitCode;
use std::sync::Arc;

use quarry_driver_mysql::MysqlDriver;

fn main() -> ExitCode {
    // A workspace build gives rustls both ring and aws-lc-rs (tiberius asks for the latter);
    // mysql_async's TLS needs one provider chosen.
    let _ = rustls::crypto::ring::default_provider().install_default();
    quarry_driver_proto::driver_main(vec![
        Arc::new(MysqlDriver::default()),
        Arc::new(MysqlDriver::mariadb()),
    ])
}
