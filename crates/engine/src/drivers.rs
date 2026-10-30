//! The drivers the engine can use: the driver binaries found on disk, each asked what it serves.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarry_driver_api::Driver;
use quarry_driver_proto::ProcessDriver;
use quarry_model::DriverId;

use crate::engine::Engine;

/// Driver binaries are named `quarry-driver-<name>`.
const PROGRAM_PREFIX: &str = "quarry-driver-";

impl Engine {
    pub(crate) fn driver(&self, id: &DriverId) -> Option<Arc<dyn Driver>> {
        self.registry
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
    }

    /// Registers every driver served by a binary in the driver directory. A binary that does
    /// not answer is skipped with a warning; the others still load.
    pub(crate) async fn load_drivers(&self) {
        let Some(dir) = self.driver_dir.clone() else {
            return;
        };
        let programs = match tokio::task::spawn_blocking(move || programs_in(&dir)).await {
            Ok(Ok(programs)) => programs,
            Ok(Err(err)) => {
                tracing::warn!(%err, "cannot read the driver directory");
                return;
            }
            Err(err) => {
                tracing::warn!(%err, "driver scan stopped");
                return;
            }
        };
        for program in programs {
            match quarry_driver_proto::hello(&program).await {
                Ok(hello) => {
                    let mut registry = self.registry.write().unwrap_or_else(|p| p.into_inner());
                    for info in hello.drivers {
                        tracing::info!(id = %info.id.0, program = %program.display(), "driver");
                        registry.register(Arc::new(ProcessDriver::new(program.clone(), info)));
                    }
                }
                Err(err) => tracing::warn!(%err, "driver skipped"),
            }
        }
    }
}

/// Why a connection cannot open when its driver is not on this computer, and what to do.
pub(crate) fn missing(driver: &DriverId) -> String {
    format!(
        "The {} driver is not on this computer. Open the connection's settings and download it.",
        driver.0
    )
}

/// The driver binaries in `dir`. Build leftovers next to them (`quarry-driver-mysql.d`) are not
/// binaries.
fn programs_in(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut programs = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(PROGRAM_PREFIX) && !name.contains('.') && entry.file_type()?.is_file() {
            programs.push(entry.path());
        }
    }
    programs.sort();
    Ok(programs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_driver_binaries_are_programs() {
        let dir = std::env::temp_dir().join(format!("quarry-drivers-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("quarry-driver-dir")).unwrap();
        for name in [
            "quarry-driver-mysql",
            "quarry-driver-mysql.d",
            "quarry",
            "other",
        ] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let names: Vec<String> = programs_in(&dir)
            .unwrap()
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
            .collect();
        assert_eq!(names, vec!["quarry-driver-mysql"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
