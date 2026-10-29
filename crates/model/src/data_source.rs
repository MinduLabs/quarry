use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::DriverId;

/// Where a data source is listed when the user picks what to connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DataSourceSection {
    Database,
    Cloud,
    Demo,
}

/// Something the user can connect to, whether or not Quarry can open it yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DataSource {
    /// Stable key; clients pick the logo by it.
    pub id: String,
    pub name: String,
    pub section: DataSourceSection,
    /// Driver a connection to it uses. `None` while Quarry has no code for it (Coming soon).
    pub driver: Option<DriverId>,
    pub default_port: Option<u16>,
    /// The driver's binary is on this machine; until then it is downloaded with `InstallDriver`.
    pub is_installed: bool,
}
