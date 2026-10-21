//! Saved connections, secrets, SSH tunnels and the connect / disconnect lifecycle.

use std::sync::Arc;

use quarry_driver_api::{ResolvedConfig, Session, SharedCancel};
use quarry_model::{ConnId, ConnectionConfig};
use quarry_store::SecretKind;
use quarry_tunnel::{Tunnel, TunnelSpec};

use crate::engine::{Engine, SharedSession, close_shared, describe, lock};
use crate::{ConnState, Event, NoticeKind, Password, TabId};

/// Which SSH tunnel `Engine::resolve` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TunnelUse {
    Shared,
    Temporary,
}

impl Engine {
    pub(crate) fn load_connections(&self) {
        let Some(store) = &self.config else {
            self.emit(Event::Connections {
                connections: Vec::new(),
            });
            return;
        };
        match store.load_connections() {
            Ok(list) => {
                *lock(&self.connections) = list.clone();
                self.emit(Event::Connections { connections: list });
            }
            Err(err) => self.notice(
                NoticeKind::Error,
                format!("Couldn't read saved connections: {err}"),
            ),
        }
    }

    pub(crate) fn persist(&self) -> Vec<ConnectionConfig> {
        let list = lock(&self.connections).clone();
        if let Some(store) = &self.config
            && let Err(err) = store.save_connections(&list)
        {
            self.notice(
                NoticeKind::Error,
                format!("Couldn't save connections: {err}"),
            );
        }
        list
    }

    pub(crate) async fn save_connection(
        &self,
        config: ConnectionConfig,
        password: Option<Password>,
        ssh_secret: Option<Password>,
        remember_password: bool,
    ) {
        let id = config.id;
        let label = config.name.clone();
        let uses_ssh = config.ssh.is_some();
        let was_connected = lock(&self.meta).contains_key(&id);
        {
            let mut list = lock(&self.connections);
            match list.iter_mut().find(|c| c.id == id) {
                Some(existing) => *existing = config,
                None => list.push(config),
            }
        }
        let list = self.persist();

        if let Some(Password(password)) = password {
            self.store_secret(
                id,
                SecretKind::Database,
                &label,
                password,
                remember_password,
            )
            .await;
        }
        if let Some(Password(secret)) = ssh_secret {
            self.store_secret(id, SecretKind::Ssh, &label, secret, remember_password)
                .await;
        } else if !uses_ssh {
            lock(&self.passwords).remove(&(id, SecretKind::Ssh));
            self.forget_secret(id, SecretKind::Ssh).await;
        }
        self.emit(Event::Connections { connections: list });

        // Settings changed: drop the old sessions so the next connect uses the new ones.
        if was_connected {
            self.disconnect(id).await;
        }
    }

    pub(crate) async fn delete_connection(&self, id: ConnId) {
        self.disconnect(id).await;
        lock(&self.connections).retain(|c| c.id != id);
        lock(&self.passwords).retain(|(conn, _), _| *conn != id);
        let list = self.persist();
        self.delete_favorites(|f| f.conn == id);
        self.forget_secret(id, SecretKind::Database).await;
        self.forget_secret(id, SecretKind::Ssh).await;
        self.emit(Event::Connections { connections: list });
    }

    /// Keep a secret for this run, and in the keyring when `remember` is set.
    pub(crate) async fn store_secret(
        &self,
        id: ConnId,
        kind: SecretKind,
        label: &str,
        secret: String,
        remember: bool,
    ) {
        lock(&self.passwords).insert((id, kind), secret.clone());
        let Some(secrets) = &self.secrets else { return };
        let result = if remember {
            secrets.set_password(id, kind, label, &secret).await
        } else {
            secrets.delete_password(id, kind).await
        };
        if let Err(err) = result {
            tracing::warn!(%err, "keyring");
            if remember {
                self.notice(
                    NoticeKind::Warning,
                    format!("Password kept for this session only — keyring unavailable: {err}"),
                );
            }
        }
    }

    pub(crate) async fn forget_secret(&self, id: ConnId, kind: SecretKind) {
        if let Some(secrets) = &self.secrets
            && let Err(err) = secrets.delete_password(id, kind).await
        {
            tracing::warn!(%err, "keyring delete");
        }
    }

    // ---------- Favorites ----------

    /// Settings ready for the driver: secrets filled in and, for a connection over SSH, the
    /// address pointed at the tunnel. `tunnel` decides which tunnel: the connection's shared one
    /// (`Shared`, opened if needed; Disconnect drops it) or one owned by the caller
    /// (`Temporary`, for Test connection).
    pub(crate) async fn resolve(
        &self,
        mut config: ConnectionConfig,
        password: Option<Password>,
        ssh_secret: Option<Password>,
        tunnel: TunnelUse,
    ) -> Result<(ResolvedConfig, Option<Arc<Tunnel>>), String> {
        let password = match password {
            Some(Password(p)) => Some(p),
            None => self.stored_secret(config.id, SecretKind::Database).await,
        };
        let Some(ssh) = config.ssh.clone() else {
            return Ok((ResolvedConfig { config, password }, None));
        };
        let id = config.id;
        let existing = match tunnel {
            TunnelUse::Shared => lock(&self.tunnels).get(&id).cloned(),
            TunnelUse::Temporary => None,
        };
        let tunnel = match existing {
            Some(tunnel) => tunnel,
            None => {
                let secret = match ssh_secret {
                    Some(Password(s)) => Some(s),
                    None => self.stored_secret(id, SecretKind::Ssh).await,
                };
                let known_hosts = self
                    .known_hosts
                    .clone()
                    .ok_or_else(|| "No home directory, so no ~/.ssh/known_hosts.".to_owned())?;
                let spec = TunnelSpec {
                    ssh,
                    secret,
                    target_host: config.host.clone().unwrap_or_else(|| "127.0.0.1".into()),
                    target_port: config
                        .port
                        .ok_or_else(|| "Set the database port to connect over SSH.".to_owned())?,
                    known_hosts,
                };
                let opened = Arc::new(quarry_tunnel::open(spec).await.map_err(|e| e.to_string())?);
                if tunnel == TunnelUse::Shared {
                    lock(&self.tunnels).insert(id, opened.clone());
                }
                opened
            }
        };
        config.host = Some("127.0.0.1".into());
        config.port = Some(tunnel.local_port());
        Ok((ResolvedConfig { config, password }, Some(tunnel)))
    }

    pub(crate) async fn stored_secret(&self, id: ConnId, kind: SecretKind) -> Option<String> {
        if let Some(p) = lock(&self.passwords).get(&(id, kind)) {
            return Some(p.clone());
        }
        let secrets = self.secrets?;
        match secrets.get_password(id, kind).await {
            Ok(Some(p)) => {
                lock(&self.passwords).insert((id, kind), p.clone());
                Some(p)
            }
            Ok(None) => None,
            Err(err) => {
                tracing::warn!(%err, "keyring read");
                None
            }
        }
    }

    pub(crate) async fn open_session(
        &self,
        resolved: &ResolvedConfig,
    ) -> Result<Box<dyn Session>, String> {
        let driver = self
            .registry
            .get(&resolved.config.driver)
            .ok_or_else(|| format!("No driver for “{}”", resolved.config.driver.0))?;
        let mut session = driver.connect(resolved).await.map_err(|e| describe(&e))?;
        if resolved.config.read_only {
            session
                .set_read_only(true)
                .await
                .map_err(|e| describe(&e))?;
        }
        Ok(session)
    }

    pub(crate) fn config_of(&self, id: ConnId) -> Option<ConnectionConfig> {
        lock(&self.connections).iter().find(|c| c.id == id).cloned()
    }

    pub(crate) async fn test_connection(
        &self,
        config: ConnectionConfig,
        password: Option<Password>,
        ssh_secret: Option<Password>,
    ) -> Result<String, String> {
        // The temporary tunnel lives until this function returns.
        let (resolved, _tunnel) = self
            .resolve(config, password, ssh_secret, TunnelUse::Temporary)
            .await?;
        let session = self.open_session(&resolved).await?;
        let version = session.server_version();
        let _ = session.close().await;
        Ok(version)
    }

    pub(crate) async fn connect(&self, id: ConnId) {
        let Some(config) = self.config_of(id) else {
            return;
        };
        self.emit(Event::ConnState {
            id,
            state: ConnState::Connecting,
        });

        let failed = |error: String| Event::ConnState {
            id,
            state: ConnState::Failed { error },
        };
        let resolved = match self.resolve(config, None, None, TunnelUse::Shared).await {
            Ok((resolved, _)) => resolved,
            Err(err) => return self.emit(failed(err)),
        };
        let mut session = match self.open_session(&resolved).await {
            Ok(session) => session,
            Err(err) => return self.emit(failed(err)),
        };
        let databases = match session.list_databases().await {
            Ok(dbs) => dbs,
            Err(err) => return self.emit(failed(describe(&err))),
        };
        let server_version = session.server_version();
        let capabilities = self
            .registry
            .get(&resolved.config.driver)
            .map(|d| d.capabilities())
            .unwrap_or_default();
        lock(&self.meta).insert(id, Arc::new(tokio::sync::Mutex::new(session)));
        self.emit(Event::ConnState {
            id,
            state: ConnState::Connected {
                server_version,
                capabilities,
            },
        });
        self.emit(Event::Databases { id, databases });
    }

    pub(crate) async fn disconnect(&self, id: ConnId) {
        // Cancel running queries on this connection first, otherwise we would wait for them.
        let running: Vec<SharedCancel> = {
            let mut cancels = lock(&self.cancels);
            let ids: Vec<TabId> = cancels
                .iter()
                .filter(|(_, (c, _))| *c == id)
                .map(|(t, _)| *t)
                .collect();
            ids.iter()
                .filter_map(|t| cancels.remove(t))
                .map(|(_, c)| c)
                .collect()
        };
        for cancel in running {
            cancel.cancel();
        }
        let meta = lock(&self.meta).remove(&id);
        let tabs: Vec<_> = lock(&self.tabs).values().cloned().collect();
        let mut sessions: Vec<SharedSession> = meta.into_iter().collect();
        for slot in tabs {
            let mut slot = slot.lock().await;
            if slot.as_ref().is_some_and(|t| t.conn == id)
                && let Some(tab) = slot.take()
            {
                sessions.push(tab.session);
            }
        }
        let had_sessions = !sessions.is_empty();
        for session in sessions {
            close_shared(session).await;
        }
        // Sessions are closed, so the tunnel can go.
        lock(&self.tunnels).remove(&id);
        if had_sessions {
            self.emit(Event::ConnState {
                id,
                state: ConnState::Disconnected,
            });
        }
    }
}
