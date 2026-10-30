//! What the user can pick when creating a connection, and which of it this build can open.

use quarry_model::{DataSource, DataSourceSection, DriverId};

use crate::Event;
use crate::engine::Engine;

struct Entry {
    id: &'static str,
    name: &'static str,
    section: DataSourceSection,
    /// Driver id once Quarry has code for it; empty while it has none (Coming soon).
    driver: &'static str,
    /// The driver binary that serves it, downloaded on first use.
    package: &'static str,
    default_port: Option<u16>,
}

/// Listed in this order. An entry with a driver can be picked; its driver is installed once its
/// binary is on this machine.
const CATALOG: &[Entry] = &[
    Entry {
        id: "mysql",
        name: "MySQL",
        section: DataSourceSection::Database,
        driver: "mysql",
        package: "quarry-driver-mysql",
        default_port: Some(3306),
    },
    Entry {
        id: "mariadb",
        name: "MariaDB",
        section: DataSourceSection::Database,
        driver: "mariadb",
        package: "quarry-driver-mysql",
        default_port: Some(3306),
    },
    Entry {
        id: "postgresql",
        name: "PostgreSQL",
        section: DataSourceSection::Database,
        driver: "postgres",
        package: "quarry-driver-postgres",
        default_port: Some(5432),
    },
    Entry {
        id: "sqlserver",
        name: "Microsoft SQL Server",
        section: DataSourceSection::Database,
        driver: "sqlserver",
        package: "quarry-driver-sqlserver",
        default_port: Some(1433),
    },
    Entry {
        id: "google-cloud",
        name: "Google Cloud",
        section: DataSourceSection::Cloud,
        driver: "",
        package: "",
        default_port: None,
    },
    Entry {
        id: "aws",
        name: "AWS",
        section: DataSourceSection::Cloud,
        driver: "",
        package: "",
        default_port: None,
    },
    Entry {
        id: "azure",
        name: "Azure",
        section: DataSourceSection::Cloud,
        driver: "",
        package: "",
        default_port: None,
    },
    Entry {
        id: "demo-postgresql",
        name: "Demo PostgreSQL",
        section: DataSourceSection::Demo,
        driver: "",
        package: "",
        default_port: None,
    },
    Entry {
        id: "demo-mysql",
        name: "Demo MySQL",
        section: DataSourceSection::Demo,
        driver: "",
        package: "",
        default_port: None,
    },
    Entry {
        id: "demo-sqlite",
        name: "Demo SQLite",
        section: DataSourceSection::Demo,
        driver: "",
        package: "",
        default_port: None,
    },
];

impl Engine {
    pub(crate) fn load_data_sources(&self) {
        let sources = CATALOG
            .iter()
            .map(|entry| {
                let driver = (!entry.driver.is_empty()).then(|| DriverId(entry.driver.to_owned()));
                DataSource {
                    id: entry.id.to_owned(),
                    name: entry.name.to_owned(),
                    section: entry.section,
                    is_installed: driver.as_ref().is_some_and(|id| self.driver(id).is_some()),
                    driver,
                    default_port: entry.default_port,
                }
            })
            .collect();
        self.emit(Event::DataSources { sources });
    }
}

/// The binary that serves `driver`, or `None` when Quarry has no driver of that id.
pub(crate) fn package_of(driver: &DriverId) -> Option<&'static str> {
    CATALOG
        .iter()
        .find(|entry| entry.driver == driver.0 && !entry.package.is_empty())
        .map(|entry| entry.package)
}
