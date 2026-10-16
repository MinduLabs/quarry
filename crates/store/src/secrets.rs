//! Connection passwords in the OS keyring.
//!
//! Key: service `quarry`, account `<connection uuid>`.

use quarry_model::ConnId;

use crate::{Result, StoreError};

const SERVICE: &str = "quarry";

#[derive(Debug, Default, Clone, Copy)]
pub struct SecretStore;

#[cfg(target_os = "linux")]
impl SecretStore {
    fn attributes(id: ConnId) -> [(&'static str, String); 2] {
        [
            ("application", SERVICE.to_owned()),
            ("connection", id.0.to_string()),
        ]
    }

    pub async fn get_password(&self, id: ConnId) -> Result<Option<String>> {
        let keyring = oo7::Keyring::new().await.map_err(keyring_err)?;
        let items = keyring
            .search_items(&Self::attributes(id))
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

    pub async fn set_password(&self, id: ConnId, label: &str, password: &str) -> Result<()> {
        let keyring = oo7::Keyring::new().await.map_err(keyring_err)?;
        keyring
            .create_item(
                &format!("Quarry: {label}"),
                &Self::attributes(id),
                oo7::Secret::text(password),
                true,
            )
            .await
            .map_err(keyring_err)
    }

    pub async fn delete_password(&self, id: ConnId) -> Result<()> {
        let keyring = oo7::Keyring::new().await.map_err(keyring_err)?;
        keyring
            .delete(&Self::attributes(id))
            .await
            .map_err(keyring_err)
    }
}

#[cfg(target_os = "macos")]
impl SecretStore {
    /// `errSecItemNotFound`
    const NOT_FOUND: i32 = -25300;

    pub async fn get_password(&self, id: ConnId) -> Result<Option<String>> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(SERVICE, &id.0.to_string()) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(err) if err.code() == Self::NOT_FOUND => Ok(None),
            Err(err) => Err(keyring_err(err)),
        }
    }

    pub async fn set_password(&self, id: ConnId, _label: &str, password: &str) -> Result<()> {
        security_framework::passwords::set_generic_password(
            SERVICE,
            &id.0.to_string(),
            password.as_bytes(),
        )
        .map_err(keyring_err)
    }

    pub async fn delete_password(&self, id: ConnId) -> Result<()> {
        match security_framework::passwords::delete_generic_password(SERVICE, &id.0.to_string()) {
            Err(err) if err.code() != Self::NOT_FOUND => Err(keyring_err(err)),
            _ => Ok(()),
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl SecretStore {
    pub async fn get_password(&self, _id: ConnId) -> Result<Option<String>> {
        Ok(None)
    }

    pub async fn set_password(&self, _id: ConnId, _label: &str, _password: &str) -> Result<()> {
        Err(StoreError::Keyring(
            "not supported on this platform".to_owned(),
        ))
    }

    pub async fn delete_password(&self, _id: ConnId) -> Result<()> {
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn keyring_err(err: impl std::fmt::Display) -> StoreError {
    StoreError::Keyring(err.to_string())
}
