//! `quarry-driver-mysql`: serves MySQL and MariaDB sessions to the engine (`quarry-driver-proto`).

use std::process::ExitCode;
use std::sync::Arc;

use quarry_driver_mysql::MysqlDriver;

fn main() -> ExitCode {
    quarry_driver_proto::driver_main(vec![
        Arc::new(MysqlDriver::default()),
        Arc::new(MysqlDriver::mariadb()),
    ])
}
