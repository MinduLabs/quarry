//! Connection secrets in the OS keyring.
//!
//! Key: service `quarry`, account `<connection uuid>` for the database password and
//! `<connection uuid>:ssh` for the SSH password or key passphrase.

use quarry_model::ConnId;

use crate::{Result, StoreError};

const SERVICE: &str = "quarry";

#[derive(Debug, Default, Clone, Copy)]
pub struct SecretStore;

/// Which secret of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKind {
    Database,
    /// SSH password or key passphrase.
    Ssh,
}

fn account(id: ConnId, kind: SecretKind) -> String {
    match kind {
        SecretKind::Database => id.0.to_string(),
        SecretKind::Ssh => format!("{}:ssh", id.0),
    }
}

#[cfg(target_os = "linux")]
impl SecretStore {
    fn attributes(id: ConnId, kind: SecretKind) -> [(&'static str, String); 2] {
        [
            ("application", SERVICE.to_owned()),
            ("connection", account(id, kind)),
        ]
    }

    pub async fn get_password(&self, id: ConnId, kind: SecretKind) -> Result<Option<String>> {
        let keyring = oo7::Keyring::new().await.map_err(keyring_err)?;
        let items = keyring
            .search_items(&Self::attributes(id, kind))
            .await
            .map_err(keyring_err)?;
        let Some(item) = items.first() else {
            return Ok(None);
        };
        let secret = item.secret().await.map_err(keyring_err)?;
        Ok(Some(
            String::from_utf8_lossy(secret.as_bytes()).into_owned(),
        ))
    }

    pub async fn set_password(
        &self,
        id: ConnId,
        kind: SecretKind,
        label: &str,
        password: &str,
    ) -> Result<()> {
        let keyring = oo7::Keyring::new().await.map_err(keyring_err)?;
        let label = match kind {
            SecretKind::Database => format!("Quarry: {label}"),
            SecretKind::Ssh => format!("Quarry: {label} (SSH)"),
        };
        keyring
            .create_item(
                &label,
                &Self::attributes(id, kind),
                oo7::Secret::text(password),
                true,
            )
            .await
            .map_err(keyring_err)
    }

    pub async fn delete_password(&self, id: ConnId, kind: SecretKind) -> Result<()> {
        let keyring = oo7::Keyring::new().await.map_err(keyring_err)?;
        keyring
            .delete(&Self::attributes(id, kind))
            .await
            .map_err(keyring_err)
    }
}

#[cfg(target_os = "macos")]
impl SecretStore {
    /// `errSecItemNotFound`
    const NOT_FOUND: i32 = -25300;

    pub async fn get_password(&self, id: ConnId, kind: SecretKind) -> Result<Option<String>> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(SERVICE, &account(id, kind)) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(err) if err.code() == Self::NOT_FOUND => Ok(None),
            Err(err) => Err(keyring_err(err)),
        }
    }

    pub async fn set_password(
        &self,
        id: ConnId,
        kind: SecretKind,
        _label: &str,
        password: &str,
    ) -> Result<()> {
        security_framework::passwords::set_generic_password(
            SERVICE,
            &account(id, kind),
            password.as_bytes(),
        )
        .map_err(keyring_err)
    }

    pub async fn delete_password(&self, id: ConnId, kind: SecretKind) -> Result<()> {
        match security_framework::passwords::delete_generic_password(SERVICE, &account(id, kind)) {
            Err(err) if err.code() != Self::NOT_FOUND => Err(keyring_err(err)),
            _ => Ok(()),
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl SecretStore {
    pub async fn get_password(&self, _id: ConnId, _kind: SecretKind) -> Result<Option<String>> {
        Ok(None)
    }

    pub async fn set_password(
        &self,
        _id: ConnId,
        _kind: SecretKind,
        _label: &str,
        _password: &str,
    ) -> Result<()> {
        Err(StoreError::Keyring(
            "not supported on this platform".to_owned(),
        ))
    }

    pub async fn delete_password(&self, _id: ConnId, _kind: SecretKind) -> Result<()> {
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn keyring_err(err: impl std::fmt::Display) -> StoreError {
    StoreError::Keyring(err.to_string())
}
