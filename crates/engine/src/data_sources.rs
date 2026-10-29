//! What the user can pick when creating a connection, and which of it this build can open.

use quarry_model::{DataSource, DataSourceSection, DriverId};

use crate::Event;
use crate::engine::Engine;

struct Entry {
    id: &'static str,
    name: &'static str,
    section: DataSourceSection,
    /// Driver id once Quarry has one; empty for entries no driver will serve directly.
    driver: &'static str,
    default_port: Option<u16>,
}

/// Listed in this order. An entry turns usable when a driver with its id is registered.
const CATALOG: &[Entry] = &[
    Entry {
        id: "mysql",
        name: "MySQL",
        section: DataSourceSection::Database,
        driver: "mysql",
        default_port: Some(3306),
    },
    Entry {
        id: "mariadb",
        name: "MariaDB",
        section: DataSourceSection::Database,
        driver: "mariadb",
        default_port: Some(3306),
    },
    Entry {
        id: "postgresql",
        name: "PostgreSQL",
        section: DataSourceSection::Database,
        driver: "postgres",
        default_port: Some(5432),
    },
    Entry {
        id: "sqlserver",
        name: "Microsoft SQL Server",
        section: DataSourceSection::Database,
        driver: "sqlserver",
        default_port: Some(1433),
    },
    Entry {
        id: "google-cloud",
        name: "Google Cloud",
        section: DataSourceSection::Cloud,
        driver: "",
        default_port: None,
    },
    Entry {
        id: "aws",
        name: "AWS",
        section: DataSourceSection::Cloud,
        driver: "",
        default_port: None,
    },
    Entry {
        id: "azure",
        name: "Azure",
        section: DataSourceSection::Cloud,
        driver: "",
        default_port: None,
    },
    Entry {
        id: "demo-postgresql",
        name: "Demo PostgreSQL",
        section: DataSourceSection::Demo,
        driver: "",
        default_port: None,
    },
    Entry {
        id: "demo-mysql",
        name: "Demo MySQL",
        section: DataSourceSection::Demo,
        driver: "",
        default_port: None,
    },
    Entry {
        id: "demo-sqlite",
        name: "Demo SQLite",
        section: DataSourceSection::Demo,
        driver: "",
        default_port: None,
    },
];

impl Engine {
    pub(crate) fn load_data_sources(&self) {
        let sources = CATALOG
            .iter()
            .map(|entry| {
                let driver = DriverId(entry.driver.to_owned());
                // Every registered driver is compiled in, so it is installed too.
                let driver = self.registry.get(&driver).map(|_| driver);
                DataSource {
                    id: entry.id.to_owned(),
                    name: entry.name.to_owned(),
                    section: entry.section,
                    is_installed: driver.is_some(),
                    driver,
                    default_port: entry.default_port,
                }
            })
            .collect();
        self.emit(Event::DataSources { sources });
    }
}
