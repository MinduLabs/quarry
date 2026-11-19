//! Which driver binaries the engine may run. A driver is a program, so a release build runs only
//! the ones its own release signed: `drivers.json` lists each binary's SHA-256, and
//! `drivers.json.sig` is its minisign signature, made with the updater's key (the `pubkey` in
//! tauri.conf.json). The signed list is kept next to the installed drivers, so every start checks
//! them again without the network: a driver of another Quarry version, or a changed file, is not
//! loaded, and the user downloads it again.
//!
//! Debug builds (`just dev`, `./qd`, tests) run the drivers they built themselves, unsigned.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use minisign_verify::{PublicKey, Signature};
use serde::Deserialize;

pub(crate) const MANIFEST_FILE: &str = "drivers.json";
pub(crate) const SIGNATURE_FILE: &str = "drivers.json.sig";

/// How strict the engine is with driver binaries.
#[derive(Debug, Clone, Default)]
pub(crate) struct DriverTrust {
    /// The minisign public key, base64 as in tauri.conf.json.
    pub key: Option<String>,
    /// A driver list must be signed and be this Quarry version's. On in release builds.
    pub required: bool,
}

/// `drivers.json`: for each driver package and target, its release file and SHA-256.
#[derive(Debug, Deserialize)]
pub(crate) struct Manifest {
    /// The Quarry version whose release it lists. Missing before signed lists existed.
    #[serde(default)]
    pub version: Option<String>,
    /// Package name → target → file.
    pub packages: HashMap<String, HashMap<String, Artifact>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Artifact {
    pub url: String,
    pub sha256: String,
}

impl Manifest {
    /// The SHA-256 the list gives for `package` on `target`.
    pub(crate) fn sha256(&self, package: &str, target: &str) -> Option<&str> {
        self.packages
            .get(package)?
            .get(target)
            .map(|a| a.sha256.as_str())
    }
}

impl DriverTrust {
    /// `bytes` as a driver list. When required, it must carry a valid `signature` and be for
    /// `version`; otherwise a signature is checked when there is one and a key to check it with.
    pub(crate) fn manifest(
        &self,
        bytes: &[u8],
        signature: Option<&str>,
        version: &str,
    ) -> Result<Manifest, String> {
        match (&self.key, signature) {
            (Some(key), Some(signature)) => verify(bytes, signature, key)?,
            (None, _) if self.required => {
                return Err(
                    "This build of Quarry has no key to check drivers with. Install Quarry from its release page."
                        .to_owned(),
                );
            }
            (Some(_), None) if self.required => {
                return Err(
                    "The list of drivers is not signed, so Quarry will not run them. Try again later, or update Quarry."
                        .to_owned(),
                );
            }
            _ => {}
        }
        let manifest: Manifest = serde_json::from_slice(bytes).map_err(|err| {
            format!("The list of drivers could not be read ({err}). Try again later.")
        })?;
        if self.required && manifest.version.as_deref() != Some(version) {
            return Err(format!(
                "The list of drivers is for Quarry {}, not {version}. Update Quarry, or download the driver again.",
                manifest.version.as_deref().unwrap_or("an older version")
            ));
        }
        Ok(manifest)
    }
}

impl DriverTrust {
    /// The `programs` in `dir` that the signed list kept there names, with a matching SHA-256, for
    /// `version` on `target`. The others (a driver of another Quarry version, a changed file) are
    /// skipped before anything runs them; the user downloads them again. Blocking IO.
    pub(crate) fn signed(
        &self,
        dir: &Path,
        programs: Vec<PathBuf>,
        version: &str,
        target: &str,
    ) -> Vec<PathBuf> {
        if programs.is_empty() {
            return programs;
        }
        let manifest = std::fs::read(dir.join(MANIFEST_FILE))
            .map_err(|err| format!("no driver list ({err})"))
            .and_then(|bytes| {
                let signature = std::fs::read_to_string(dir.join(SIGNATURE_FILE)).ok();
                self.manifest(&bytes, signature.as_deref(), version)
            });
        let manifest = match manifest {
            Ok(manifest) => manifest,
            Err(err) => {
                tracing::warn!(%err, "drivers skipped: no signed list for this version");
                return Vec::new();
            }
        };
        programs
            .into_iter()
            .filter(|program| {
                let name = program.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let is_signed = manifest.sha256(name, target).is_some_and(|want| {
                    std::fs::read(program)
                        .is_ok_and(|bytes| sha256_hex(&bytes).eq_ignore_ascii_case(want))
                });
                if !is_signed {
                    tracing::warn!(
                        program = %program.display(),
                        "driver skipped: not in this version's signed list; download it again"
                    );
                }
                is_signed
            })
            .collect()
    }
}

/// Check `data` against a minisign `signature` from `key`, both base64 as Tauri writes them.
fn verify(data: &[u8], signature: &str, key: &str) -> Result<(), String> {
    let refused = |why: &dyn std::fmt::Display| {
        format!(
            "The list of drivers failed its signature check ({why}), so Quarry will not run them. Try again later, or update Quarry."
        )
    };
    let text = |b64: &str| {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| refused(&e))?;
        String::from_utf8(bytes).map_err(|e| refused(&e))
    };
    let key = PublicKey::decode(&text(key)?).map_err(|e| refused(&e))?;
    let signature = Signature::decode(&text(signature)?).map_err(|e| refused(&e))?;
    key.verify(data, &signature, true).map_err(|e| refused(&e))
}

/// Lowercase hex SHA-256 of `bytes`.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A throwaway key pair made with `tauri signer generate` for these tests only, and
    /// `drivers.json` signed with `tauri signer sign`. It lists `quarry-driver-mysql` with the
    /// SHA-256 of the bytes `driver`, for Quarry 9.9.9.
    pub(crate) const KEY: &str = include_str!("../testdata/signed-drivers/key.pub");
    pub(crate) const MANIFEST: &str = include_str!("../testdata/signed-drivers/drivers.json");
    pub(crate) const SIGNATURE: &str = include_str!("../testdata/signed-drivers/drivers.json.sig");

    fn strict() -> DriverTrust {
        DriverTrust {
            key: Some(KEY.to_owned()),
            required: true,
        }
    }

    #[test]
    fn a_signed_list_of_this_version_is_trusted() {
        let manifest = strict()
            .manifest(MANIFEST.as_bytes(), Some(SIGNATURE), "9.9.9")
            .unwrap();
        assert_eq!(
            manifest.sha256("quarry-driver-mysql", "x86_64-unknown-linux-gnu"),
            Some(sha256_hex(b"driver").as_str())
        );
    }

    #[test]
    fn a_changed_list_fails_its_signature() {
        let changed = MANIFEST.replace("example.invalid", "evil.example");
        let err = strict()
            .manifest(changed.as_bytes(), Some(SIGNATURE), "9.9.9")
            .unwrap_err();
        assert!(err.contains("signature check"), "{err}");
    }

    #[test]
    fn a_list_without_signature_or_of_another_version_is_refused() {
        let unsigned = strict()
            .manifest(MANIFEST.as_bytes(), None, "9.9.9")
            .unwrap_err();
        assert!(unsigned.contains("not signed"), "{unsigned}");
        let other = strict()
            .manifest(MANIFEST.as_bytes(), Some(SIGNATURE), "1.0.0")
            .unwrap_err();
        assert!(other.contains("for Quarry 9.9.9, not 1.0.0"), "{other}");
        let keyless = DriverTrust {
            key: None,
            required: true,
        };
        assert!(
            keyless
                .manifest(MANIFEST.as_bytes(), Some(SIGNATURE), "9.9.9")
                .is_err()
        );
    }

    /// A driver folder holding the signed list and `files` (name, contents).
    fn driver_dir(files: &[(&str, &[u8])], signature: Option<&str>) -> (PathBuf, Vec<PathBuf>) {
        let dir = std::env::temp_dir().join(format!("quarry-trust-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE), MANIFEST).unwrap();
        if let Some(signature) = signature {
            std::fs::write(dir.join(SIGNATURE_FILE), signature).unwrap();
        }
        let programs = files
            .iter()
            .map(|(name, bytes)| {
                let path = dir.join(name);
                std::fs::write(&path, bytes).unwrap();
                path
            })
            .collect();
        (dir, programs)
    }

    const LINUX: &str = "x86_64-unknown-linux-gnu";

    #[test]
    fn at_start_only_drivers_matching_the_signed_list_load() {
        let (dir, programs) = driver_dir(
            &[
                ("quarry-driver-mysql", b"driver"),
                ("quarry-driver-postgres", b"driver"),
            ],
            Some(SIGNATURE),
        );
        let loaded = strict().signed(&dir, programs, "9.9.9", LINUX);
        assert_eq!(loaded, vec![dir.join("quarry-driver-mysql")]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn at_start_a_changed_driver_does_not_load() {
        let (dir, programs) = driver_dir(&[("quarry-driver-mysql", b"tampered")], Some(SIGNATURE));
        assert!(strict().signed(&dir, programs, "9.9.9", LINUX).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn at_start_drivers_of_another_version_or_without_signature_do_not_load() {
        let (dir, programs) = driver_dir(&[("quarry-driver-mysql", b"driver")], Some(SIGNATURE));
        assert!(strict().signed(&dir, programs, "1.0.0", LINUX).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
        let (dir, programs) = driver_dir(&[("quarry-driver-mysql", b"driver")], None);
        assert!(strict().signed(&dir, programs, "9.9.9", LINUX).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn debug_builds_read_an_unsigned_list() {
        let lax = DriverTrust::default();
        let manifest = lax.manifest(MANIFEST.as_bytes(), None, "1.0.0").unwrap();
        assert_eq!(manifest.version.as_deref(), Some("9.9.9"));
    }
}
