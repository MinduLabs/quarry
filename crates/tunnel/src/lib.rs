//! SSH tunnels for database connections: local port forwarding via `russh`.
//!
//! [`open`] signs in to the SSH server, listens on `127.0.0.1:<random port>` and forwards every
//! connection made there to the database host as seen from the SSH server. The driver then
//! connects to the local port. Dropping the [`Tunnel`] stops it.
//!
//! The server key must already be in `known_hosts`: an unknown or changed key is refused, and the
//! error tells the user to check it once with `ssh`. Sign-in: ssh-agent, a key file (optional
//! passphrase) or a password.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quarry_model::{SshAuth, SshConfig};
use russh::client::{self, Handle};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum TunnelError {
    /// Every message is ready to show: what happened, then what to do, then a `Code:` line.
    #[error("{0}")]
    Message(String),
}

fn message(headline: impl Into<String>, why: &str, code: impl std::fmt::Display) -> TunnelError {
    TunnelError::Message(format!("{}\n{why}\nCode: {code}", headline.into()))
}

/// Where the tunnel goes, and how to sign in.
#[derive(Clone)]
pub struct TunnelSpec {
    pub ssh: SshConfig,
    /// SSH password, or the key file passphrase.
    pub secret: Option<String>,
    /// Database host and port, as reached from the SSH server.
    pub target_host: String,
    pub target_port: u16,
    /// `~/.ssh/known_hosts` unless a test points elsewhere.
    pub known_hosts: PathBuf,
}

// Never print the secret.
impl std::fmt::Debug for TunnelSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TunnelSpec")
            .field("ssh", &self.ssh)
            .field("secret", &self.secret.as_ref().map(|_| "***"))
            .field("target_host", &self.target_host)
            .field("target_port", &self.target_port)
            .finish()
    }
}

/// A running tunnel. The forwarding stops when it is dropped.
#[derive(Debug)]
pub struct Tunnel {
    local_port: u16,
    task: JoinHandle<()>,
}

impl Tunnel {
    /// Connect the database driver to `127.0.0.1` on this port.
    pub fn local_port(&self) -> u16 {
        self.local_port
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The user's `~/.ssh/known_hosts`.
pub fn default_known_hosts() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join(".ssh").join("known_hosts"))
}

/// What the host key check found, kept for the error message.
#[derive(Debug, Clone)]
enum HostKey {
    Unknown(String),
    Changed(String),
    Unsupported,
}

struct Client {
    host: String,
    port: u16,
    known_hosts: PathBuf,
    rejected: Arc<Mutex<Option<HostKey>>>,
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key else {
            *self.rejected.lock().unwrap_or_else(|p| p.into_inner()) = Some(HostKey::Unsupported);
            return Ok(false);
        };
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let verdict =
            russh::keys::check_known_hosts_path(&self.host, self.port, key, &self.known_hosts);
        let rejected = match verdict {
            Ok(true) => return Ok(true),
            Err(russh::keys::Error::KeyChanged { .. }) => HostKey::Changed(fingerprint),
            // No entry, or no readable known_hosts file.
            Ok(false) | Err(_) => HostKey::Unknown(fingerprint),
        };
        *self.rejected.lock().unwrap_or_else(|p| p.into_inner()) = Some(rejected);
        Ok(false)
    }
}

pub async fn open(spec: TunnelSpec) -> Result<Tunnel, TunnelError> {
    let ssh = &spec.ssh;
    let address = format!("{}:{}", ssh.host, ssh.port);
    let rejected = Arc::new(Mutex::new(None));
    let handler = Client {
        host: ssh.host.clone(),
        port: ssh.port,
        known_hosts: spec.known_hosts.clone(),
        rejected: rejected.clone(),
    };
    let config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        ..client::Config::default()
    });

    let connecting = client::connect(config, (ssh.host.as_str(), ssh.port), handler);
    let mut handle = match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
        Err(_) => {
            return Err(message(
                format!("Couldn't reach the SSH server {address}"),
                "No answer in time. Check the SSH host and port, and your network or VPN.",
                "ETIMEDOUT (SSH connect timed out)",
            ));
        }
        Ok(Ok(handle)) => handle,
        Ok(Err(err)) => {
            let rejected = rejected.lock().unwrap_or_else(|p| p.into_inner()).clone();
            return Err(match rejected {
                Some(HostKey::Unknown(fingerprint)) => message(
                    format!("The SSH server {address} is not a known host"),
                    &format!(
                        "Quarry only connects to servers listed in ~/.ssh/known_hosts. Check the \
                         server once in a terminal with `ssh -p {} {}@{}` and accept its key, \
                         then try again.",
                        ssh.port, ssh.user, ssh.host
                    ),
                    format!("host key {fingerprint}"),
                ),
                Some(HostKey::Changed(fingerprint)) => message(
                    format!("The SSH server {address} has a different key than before"),
                    "This can mean someone is intercepting the connection. Ask the server's \
                     admin whether the key changed; if it did, update ~/.ssh/known_hosts.",
                    format!("host key {fingerprint}"),
                ),
                Some(HostKey::Unsupported) => message(
                    format!("The SSH server {address} uses a host certificate"),
                    "Host certificates are not supported yet.",
                    "host certificate",
                ),
                None => message(
                    format!("Couldn't connect to the SSH server {address}"),
                    "Check the SSH host and port.",
                    err,
                ),
            });
        }
    };

    authenticate(&mut handle, &spec).await?;

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| message("Couldn't start the SSH tunnel", "No local port free.", e))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| message("Couldn't start the SSH tunnel", "No local port free.", e))?
        .port();
    let task = tokio::spawn(forward(
        listener,
        Arc::new(handle),
        spec.target_host.clone(),
        spec.target_port,
    ));
    tracing::info!(ssh = %address, local_port, "SSH tunnel open");
    Ok(Tunnel { local_port, task })
}

async fn authenticate(handle: &mut Handle<Client>, spec: &TunnelSpec) -> Result<(), TunnelError> {
    let ssh = &spec.ssh;
    let denied = |why: &str| {
        message(
            format!("The SSH server refused the sign-in for {}", ssh.user),
            why,
            "SSH authentication failed",
        )
    };
    let failed = |e: &dyn std::fmt::Display| {
        message(
            "SSH sign-in failed",
            "The connection broke while signing in.",
            e,
        )
    };

    let success = match ssh.auth {
        SshAuth::Password => {
            let Some(password) = &spec.secret else {
                return Err(denied("Enter the SSH password in the connection settings."));
            };
            handle
                .authenticate_password(&ssh.user, password)
                .await
                .map_err(|e| failed(&e))?
                .success()
        }
        SshAuth::Key => {
            let path = ssh
                .key_path
                .as_deref()
                .filter(|p| !p.trim().is_empty())
                .ok_or_else(|| denied("Choose a private key file in the connection settings."))?;
            let path = expand_home(path);
            let key = russh::keys::load_secret_key(&path, spec.secret.as_deref()).map_err(|e| {
                message(
                    format!("Couldn't read the SSH key {}", path.display()),
                    "Check the file path. If the key has a passphrase, enter it in the \
                     connection settings.",
                    e,
                )
            })?;
            let hash = handle
                .best_supported_rsa_hash()
                .await
                .map_err(|e| failed(&e))?
                .flatten();
            handle
                .authenticate_publickey(&ssh.user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await
                .map_err(|e| failed(&e))?
                .success()
        }
        SshAuth::Agent => authenticate_with_agent(handle, &ssh.user).await?,
    };
    if success {
        Ok(())
    } else {
        Err(denied(
            "Check the SSH user name and the password or key for that user.",
        ))
    }
}

#[cfg(unix)]
async fn authenticate_with_agent(
    handle: &mut Handle<Client>,
    user: &str,
) -> Result<bool, TunnelError> {
    use russh::keys::agent::AgentIdentity;
    use russh::keys::agent::client::AgentClient;

    let no_agent = |e: &dyn std::fmt::Display| {
        message(
            "Couldn't use the SSH agent",
            "Start ssh-agent and add your key (ssh-add), or choose a key file instead.",
            e,
        )
    };
    let mut agent = AgentClient::connect_env().await.map_err(|e| no_agent(&e))?;
    let identities = agent.request_identities().await.map_err(|e| no_agent(&e))?;
    let hash = handle
        .best_supported_rsa_hash()
        .await
        .map_err(|e| no_agent(&e))?
        .flatten();
    for identity in identities {
        let AgentIdentity::PublicKey { key, .. } = identity else {
            continue;
        };
        match handle
            .authenticate_publickey_with(user, key, hash, &mut agent)
            .await
        {
            Ok(result) if result.success() => return Ok(true),
            Ok(_) => {}
            Err(err) => tracing::debug!(?err, "agent key refused"),
        }
    }
    Ok(false)
}

#[cfg(not(unix))]
async fn authenticate_with_agent(_: &mut Handle<Client>, _: &str) -> Result<bool, TunnelError> {
    Err(message(
        "Couldn't use the SSH agent",
        "The SSH agent is not supported on this platform. Choose a key file instead.",
        "no agent",
    ))
}

/// Accept local connections and pipe each through its own SSH channel.
async fn forward(
    listener: TcpListener,
    handle: Arc<Handle<Client>>,
    target_host: String,
    target_port: u16,
) {
    loop {
        let (mut socket, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(err) => {
                tracing::warn!(%err, "tunnel accept");
                continue;
            }
        };
        let handle = handle.clone();
        let host = target_host.clone();
        tokio::spawn(async move {
            let channel = handle
                .channel_open_direct_tcpip(
                    host,
                    u32::from(target_port),
                    "127.0.0.1",
                    u32::from(peer.port()),
                )
                .await;
            match channel {
                Ok(channel) => {
                    let mut stream = channel.into_stream();
                    if let Err(err) = tokio::io::copy_bidirectional(&mut socket, &mut stream).await
                    {
                        tracing::debug!(%err, "tunnel stream closed");
                    }
                }
                Err(err) => tracing::warn!(%err, "tunnel channel"),
            }
        });
    }
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), directories::BaseDirs::new()) {
        (Some(rest), Some(dirs)) => dirs.home_dir().join(rest),
        _ => Path::new(path).to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_the_home_directory() {
        let home = directories::BaseDirs::new().unwrap().home_dir().to_owned();
        assert_eq!(
            expand_home("~/.ssh/id_ed25519"),
            home.join(".ssh/id_ed25519")
        );
        assert_eq!(expand_home("/etc/key"), PathBuf::from("/etc/key"));
    }

    #[test]
    fn debug_hides_the_secret() {
        let spec = TunnelSpec {
            ssh: SshConfig {
                host: "bastion".into(),
                port: 22,
                user: "me".into(),
                auth: SshAuth::Password,
                key_path: None,
            },
            secret: Some("hunter2".into()),
            target_host: "db".into(),
            target_port: 3306,
            known_hosts: PathBuf::from("/dev/null"),
        };
        assert!(!format!("{spec:?}").contains("hunter2"));
    }
}
