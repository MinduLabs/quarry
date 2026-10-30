//! Driver binaries are downloaded on first use: the release's `drivers.json` names a file and
//! its SHA-256 for each driver and machine; the file is checked, asked what it serves, and only
//! then put in the driver directory and loaded.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::TryStreamExt;
use quarry_driver_proto::ProcessDriver;
use quarry_model::DriverId;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::data_sources::package_of;
use crate::engine::Engine;
use crate::{DriverInstallState, Event};

/// The manifest of this version's release. `QUARRY_DRIVER_MANIFEST` points elsewhere (tests, a
/// mirror).
const MANIFEST_URL: &str = concat!(
    "https://github.com/MinduLabs/quarry/releases/download/v",
    env!("CARGO_PKG_VERSION"),
    "/drivers.json"
);

/// Progress is reported at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

#[derive(Debug, Deserialize)]
struct Manifest {
    /// Package name → target → file.
    packages: HashMap<String, HashMap<String, Artifact>>,
}

#[derive(Debug, Deserialize)]
struct Artifact {
    url: String,
    sha256: String,
}

/// The release target this build runs on; macOS packages are universal.
fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64" | "aarch64") => Some("universal-apple-darwin"),
        _ => None,
    }
}

fn http_client() -> Result<reqwest::Client, String> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|err| err.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("Quarry/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| err.to_string())
}

impl Engine {
    pub(crate) async fn install_driver(&self, driver: DriverId) {
        let state = match self.install(&driver).await {
            Ok(()) => DriverInstallState::Installed,
            Err(error) => {
                tracing::warn!(driver = %driver.0, %error, "driver install failed");
                DriverInstallState::Failed { error }
            }
        };
        self.emit(Event::DriverInstall { driver, state });
        self.load_data_sources();
    }

    async fn install(&self, driver: &DriverId) -> Result<(), String> {
        let package =
            package_of(driver).ok_or_else(|| format!("Quarry has no driver “{}”.", driver.0))?;
        let dir = self
            .driver_dir
            .clone()
            .ok_or("There is no data folder to keep drivers in. Check that your home folder can be written.")?;
        let target = target().ok_or_else(|| {
            format!(
                "Drivers are not built for this system ({} on {}).",
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        })?;
        let client = http_client()?;

        let manifest_url = self
            .driver_manifest
            .clone()
            .or_else(|| std::env::var("QUARRY_DRIVER_MANIFEST").ok())
            .unwrap_or_else(|| MANIFEST_URL.to_owned());
        let manifest: Manifest = serde_json::from_slice(
            &client
                .get(&manifest_url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|err| unreachable_release(&err))?
                .bytes()
                .await
                .map_err(|err| unreachable_release(&err))?,
        )
        .map_err(|err| {
            format!("The list of drivers could not be read ({err}). Try again later.")
        })?;
        let artifact = manifest
            .packages
            .get(package)
            .and_then(|targets| targets.get(target))
            .ok_or_else(|| format!("This driver is not available for {target} yet."))?;

        tokio::fs::create_dir_all(&dir).await.map_err(|err| {
            format!(
                "The driver folder {} could not be created: {err}.",
                dir.display()
            )
        })?;
        // Two installs of one package (MySQL and MariaDB share one) never share a file.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let staging = dir.join(format!(
            ".{package}.{}.{}.part",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let installed = dir.join(package);
        let result = self
            .fetch_and_check(&client, artifact, &staging, driver, &installed)
            .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&staging).await;
        }
        result
    }

    async fn fetch_and_check(
        &self,
        client: &reqwest::Client,
        artifact: &Artifact,
        staging: &Path,
        driver: &DriverId,
        installed: &Path,
    ) -> Result<(), String> {
        let response = client
            .get(&artifact.url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|err| unreachable_release(&err))?;
        let total = response.content_length();
        let progress = |received: u64| Event::DriverInstall {
            driver: driver.clone(),
            state: DriverInstallState::Downloading { received, total },
        };
        self.emit(progress(0));
        let mut file = tokio::fs::File::create(staging)
            .await
            .map_err(|err| format!("The driver could not be saved: {err}."))?;
        let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
        let mut received: u64 = 0;
        let mut reported = Instant::now();
        let mut body = response.bytes_stream();
        while let Some(chunk) = body
            .try_next()
            .await
            .map_err(|err| format!("The download stopped ({err}). Try again."))?
        {
            digest.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|err| format!("The driver could not be saved: {err}."))?;
            received += chunk.len() as u64;
            if reported.elapsed() >= PROGRESS_EVERY {
                reported = Instant::now();
                self.emit(progress(received));
            }
        }
        file.flush()
            .await
            .map_err(|err| format!("The driver could not be saved: {err}."))?;
        drop(file);
        self.emit(progress(received));
        self.emit(Event::DriverInstall {
            driver: driver.clone(),
            state: DriverInstallState::Verifying,
        });

        let sum: String = digest
            .finish()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if !sum.eq_ignore_ascii_case(&artifact.sha256) {
            return Err(
                "The download was damaged (its checksum does not match). Try again.".to_owned(),
            );
        }
        make_executable(staging).await?;

        // Asked before it replaces anything: a file that does not run, or serves something else,
        // never becomes the installed driver.
        let hello = quarry_driver_proto::hello(staging).await?;
        if !hello.drivers.iter().any(|info| &info.id == driver) {
            return Err(format!(
                "The downloaded file does not serve the {} driver.",
                driver.0
            ));
        }
        tokio::fs::rename(staging, installed)
            .await
            .map_err(|err| format!("The driver could not be put in place: {err}."))?;

        let mut registry = self.registry.write().unwrap_or_else(|p| p.into_inner());
        for info in hello.drivers {
            registry.register(Arc::new(ProcessDriver::new(installed.to_path_buf(), info)));
        }
        Ok(())
    }
}

fn unreachable_release(err: &reqwest::Error) -> String {
    match err.status() {
        Some(status) => format!(
            "The driver could not be downloaded (HTTP {status}). Try again later, or update Quarry."
        ),
        None => format!(
            "The driver could not be downloaded ({err}). Check your internet connection and try again."
        ),
    }
}

#[cfg(unix)]
async fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .await
        .map_err(|err| format!("The driver could not be made runnable: {err}."))
}

#[cfg(not(unix))]
async fn make_executable(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use quarry_model::DriverId;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::target;
    use crate::{Command, DriverInstallState, EngineEvents, EngineHandle, EngineOptions, Event};

    /// Serves `files` by path over HTTP on `port`, one request per connection.
    async fn serve_on(port: u16, files: HashMap<String, Vec<u8>>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let files = files.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buf = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&buf[..n]);
                    }
                    let line = String::from_utf8_lossy(&request);
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                    let response = match files.get(&path) {
                        Some(body) => {
                            let mut r = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            )
                            .into_bytes();
                            r.extend_from_slice(body);
                            r
                        }
                        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
                    };
                    let _ = socket.write_all(&response).await;
                });
            }
        });
    }

    fn sha256(bytes: &[u8]) -> String {
        ring::digest::digest(&ring::digest::SHA256, bytes)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn manifest(base: &str, sha: &str) -> Vec<u8> {
        serde_json::json!({
            "packages": {
                "quarry-driver-mysql": {
                    target().unwrap(): { "url": format!("{base}/mysql"), "sha256": sha }
                }
            }
        })
        .to_string()
        .into_bytes()
    }

    fn engine(drivers: &Path, manifest: String) -> (EngineHandle, EngineEvents) {
        let dir = std::env::temp_dir().join(format!("quarry-install-{}", uuid::Uuid::new_v4()));
        EngineHandle::spawn_with(EngineOptions {
            config_dir: Some(dir.clone()),
            data_dir: Some(dir),
            known_hosts: None,
            disable_keyring: true,
            perf_log: None,
            driver_dir: Some(drivers.to_path_buf()),
            driver_manifest: Some(manifest),
            in_process_drivers: Default::default(),
        })
        .unwrap()
    }

    /// The final state, and the progress states before it in order.
    async fn outcome(events: &EngineEvents) -> (DriverInstallState, Vec<DriverInstallState>) {
        let wait = async {
            let mut progress = Vec::new();
            loop {
                if let Event::DriverInstall { state, .. } = events.recv().await.unwrap() {
                    match state {
                        DriverInstallState::Downloading { .. } | DriverInstallState::Verifying => {
                            progress.push(state)
                        }
                        done => return (done, progress),
                    }
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(20), wait)
            .await
            .unwrap()
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok()?.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Serves `files` (the manifest names the server as `BASE`) and installs the MySQL driver.
    async fn install(
        files: impl FnOnce(&str) -> HashMap<String, Vec<u8>>,
    ) -> (
        DriverInstallState,
        PathBuf,
        EngineEvents,
        EngineHandle,
        Vec<DriverInstallState>,
    ) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let base = format!("http://127.0.0.1:{port}");
        let files = files(&base);
        serve_on(port, files).await;
        let drivers = std::env::temp_dir().join(format!("quarry-drivers-{}", uuid::Uuid::new_v4()));
        let (engine, events) = engine(&drivers, format!("{base}/drivers.json"));
        engine
            .send(Command::InstallDriver {
                driver: DriverId("mysql".into()),
            })
            .unwrap();
        let (state, progress) = outcome(&events).await;
        (state, drivers, events, engine, progress)
    }

    #[tokio::test]
    async fn a_damaged_download_is_refused_and_removed() {
        let (state, drivers, ..) = install(|base| {
            HashMap::from([
                (
                    "/drivers.json".to_owned(),
                    manifest(base, &sha256(b"other")),
                ),
                ("/mysql".to_owned(), b"#!/bin/sh\n".to_vec()),
            ])
        })
        .await;
        assert!(
            matches!(&state, DriverInstallState::Failed { error } if error.contains("checksum")),
            "{state:?}"
        );
        assert!(leftovers(&drivers).is_empty(), "{:?}", leftovers(&drivers));
    }

    #[tokio::test]
    async fn a_download_reports_its_bytes_then_verifies() {
        let body = vec![7u8; 64 * 1024];
        let sha = sha256(&body);
        let size = body.len() as u64;
        let (state, .., progress) = install(move |base| {
            HashMap::from([
                ("/drivers.json".to_owned(), manifest(base, &sha)),
                ("/mysql".to_owned(), body),
            ])
        })
        .await;
        // The bytes are not a program, so it fails after the checks start.
        assert!(
            matches!(state, DriverInstallState::Failed { .. }),
            "{state:?}"
        );
        let total = Some(size);
        assert_eq!(
            progress.first(),
            Some(&DriverInstallState::Downloading { received: 0, total })
        );
        let len = progress.len();
        assert!(len >= 3, "{progress:?}");
        assert_eq!(
            progress[len - 2],
            DriverInstallState::Downloading {
                received: size,
                total
            }
        );
        assert_eq!(progress[len - 1], DriverInstallState::Verifying);
    }

    #[tokio::test]
    async fn a_file_that_does_not_run_is_never_installed() {
        let body = b"not a program".to_vec();
        let sha = sha256(&body);
        let (state, drivers, ..) = install(move |base| {
            HashMap::from([
                ("/drivers.json".to_owned(), manifest(base, &sha)),
                ("/mysql".to_owned(), body),
            ])
        })
        .await;
        assert!(
            matches!(state, DriverInstallState::Failed { .. }),
            "{state:?}"
        );
        assert!(leftovers(&drivers).is_empty(), "{:?}", leftovers(&drivers));
    }

    #[tokio::test]
    async fn a_missing_release_says_so() {
        let (state, ..) = install(|_| HashMap::new()).await;
        assert!(
            matches!(&state, DriverInstallState::Failed { error } if error.contains("HTTP 404")),
            "{state:?}"
        );
    }

    /// The real MySQL driver binary, downloaded, checked and loaded. Runs where
    /// `QUARRY_DRIVER_DIR` holds the built binaries (`just test-db`).
    #[tokio::test]
    async fn a_real_driver_installs_and_is_offered() {
        let Some(built) = std::env::var_os("QUARRY_DRIVER_DIR")
            .map(|dir| PathBuf::from(dir).join("quarry-driver-mysql"))
            .filter(|path| path.exists())
        else {
            return;
        };
        let body = std::fs::read(built).unwrap();
        let sha = sha256(&body);
        let (state, drivers, events, engine, _) = install(move |base| {
            HashMap::from([
                ("/drivers.json".to_owned(), manifest(base, &sha)),
                ("/mysql".to_owned(), body),
            ])
        })
        .await;
        assert_eq!(state, DriverInstallState::Installed);
        assert_eq!(leftovers(&drivers), vec!["quarry-driver-mysql"]);
        let Event::DataSources { sources } = events.recv().await.unwrap() else {
            panic!("data sources follow the install");
        };
        let installed = |id: &str| sources.iter().find(|s| s.id == id).unwrap().is_installed;
        // One binary serves both.
        assert!(installed("mysql") && installed("mariadb"));
        assert!(!installed("postgresql"));
        drop(engine);
    }
}
