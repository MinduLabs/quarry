//! SSH tunnels for database connections: local port forwarding via `russh`.
//!
//! [`open`] signs in to the SSH server, through jump hosts first when there are any, listens on
//! `127.0.0.1:<random port>` and forwards every connection made there to the database host as
//! seen from the SSH server. The driver then connects to the local port. Dropping the [`Tunnel`]
//! stops it. When an SSH connection ends (server restart, network, keepalive unanswered),
//! [`Tunnel::is_alive`] turns false and the caller opens a new one.
//!
//! The SSH host may be a `~/.ssh/config` alias: its `HostName`, `Port`, `User`, `IdentityFile`
//! and `ProxyJump` fill what the connection leaves empty (`config`).
//!
//! Every server's key is checked against `known_hosts`. A changed key is refused. An unknown one
//! is refused with [`TunnelError::UnknownHost`], which carries the key, so the user can compare
//! its fingerprint and [`UnknownHost::trust`] it. Sign-in: ssh-agent, a key file (optional
//! passphrase) or a password. A password is only ever sent to the SSH server itself, never to a
//! jump host.

mod config;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quarry_model::{SshAuth, SshConfig};
use russh::client::{self, Handle};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const KEEPALIVE: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum TunnelError {
    /// Every message is ready to show: what happened, then what to do, then a `Code:` line.
    #[error("{0}")]
    Message(String),
    /// A server not in `known_hosts`. The user may compare its fingerprint and trust it.
    #[error("{}", .0.message())]
    UnknownHost(Box<UnknownHost>),
}

fn message(headline: impl Into<String>, why: &str, code: impl std::fmt::Display) -> TunnelError {
    TunnelError::Message(format!("{}\n{why}\nCode: {code}", headline.into()))
}

/// A server whose key is not in `known_hosts` yet, with the key it showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownHost {
    /// The name the key is recorded under: the alias's `HostName`, else the host as entered.
    pub host: String,
    pub port: u16,
    /// `SHA256:…`, as `ssh-keygen -lf` prints it.
    pub fingerprint: String,
    /// The key's algorithm, such as `ssh-ed25519`.
    pub key_type: String,
    key: PublicKey,
}

impl UnknownHost {
    fn new(host: &str, port: u16, key: &PublicKey) -> Self {
        Self {
            host: host.to_owned(),
            port,
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
            key_type: key.algorithm().to_string(),
            key: key.clone(),
        }
    }

    fn message(&self) -> String {
        format!(
            "The SSH server {}:{} is not a known host\nQuarry has not connected to it before. \
             Compare its key fingerprint with the one the server's admin gives you, and trust it \
             only if they match.\nCode: host key {}",
            self.host, self.port, self.fingerprint
        )
    }

    /// Record the key in `known_hosts`, so the next connection accepts this server.
    pub fn trust(&self, known_hosts: &Path) -> Result<(), TunnelError> {
        russh::keys::known_hosts::learn_known_hosts_path(
            &self.host,
            self.port,
            &self.key,
            known_hosts,
        )
        .map_err(|e| {
            message(
                format!("Couldn't save the key of {}:{}", self.host, self.port),
                &format!("Check that {} can be written.", known_hosts.display()),
                e,
            )
        })
    }
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
    /// `~/.ssh/config`; `None` reads no config.
    pub ssh_config: Option<PathBuf>,
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
pub struct Tunnel {
    local_port: u16,
    handle: Arc<Handle<Client>>,
    /// The jump host sessions the last one runs through, kept open with it.
    jumps: Vec<Arc<Handle<Client>>>,
    task: JoinHandle<()>,
}

impl Tunnel {
    /// Connect the database driver to `127.0.0.1` on this port.
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    /// Whether the SSH connection still carries new connections. A closed one never comes back.
    pub fn is_alive(&self) -> bool {
        !self.handle.is_closed()
            && self.jumps.iter().all(|j| !j.is_closed())
            && !self.task.is_finished()
    }
}

impl std::fmt::Debug for Tunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tunnel")
            .field("local_port", &self.local_port)
            .field("jumps", &self.jumps.len())
            .field("is_alive", &self.is_alive())
            .finish()
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

/// The user's `~/.ssh/config`.
pub fn default_ssh_config() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join(".ssh").join("config"))
}

/// One SSH server on the way: a jump host or the SSH server itself.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hop {
    host: String,
    port: u16,
    user: String,
    /// The alias's `IdentityFile`s, home expanded.
    identities: Vec<PathBuf>,
}

impl Hop {
    fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// `alias` resolved through the config. `port` and `user` given here win over the config's.
fn resolve(
    alias: &str,
    port: Option<u16>,
    user: Option<&str>,
    ssh_config: Option<&Path>,
) -> Result<Hop, TunnelError> {
    let settings = ssh_config
        .map(|path| config::lookup(path, alias))
        .unwrap_or_default();
    let user = user
        .filter(|u| !u.trim().is_empty())
        .map(str::to_owned)
        .or(settings.user)
        .or_else(local_user)
        .ok_or_else(|| {
            message(
                format!("No SSH user for {alias}"),
                "Enter the SSH user in the connection settings.",
                "no user",
            )
        })?;
    Ok(Hop {
        host: settings.host_name.unwrap_or_else(|| alias.to_owned()),
        port: port.or(settings.port).unwrap_or(22),
        user,
        identities: settings
            .identity_files
            .iter()
            .map(|f| expand_home(f))
            .collect(),
    })
}

fn local_user() -> Option<String> {
    ["USER", "LOGNAME", "USERNAME"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
}

/// The jump hosts, then the SSH server.
fn plan(spec: &TunnelSpec) -> Result<(Vec<Hop>, Hop), TunnelError> {
    let ssh = &spec.ssh;
    let ssh_config = spec.ssh_config.as_deref();
    // 22 is the form's default, so the config may name another port.
    let port = (ssh.port != 22).then_some(ssh.port);
    let target = resolve(&ssh.host, port, Some(&ssh.user), ssh_config)?;
    let jump = match ssh.jump.as_deref().map(str::trim).filter(|j| !j.is_empty()) {
        Some(jump) => Some(jump.to_owned()),
        None => ssh_config.and_then(|path| config::lookup(path, &ssh.host).proxy_jump),
    }
    // As in OpenSSH, "none" turns jumping off, also the alias's.
    .filter(|j| !j.eq_ignore_ascii_case("none"));
    let jumps = match jump {
        Some(jump) => jump
            .split(',')
            .map(str::trim)
            .filter(|j| !j.is_empty())
            .map(|j| {
                let (user, host, port) = parse_jump(j)?;
                resolve(host, port, user, ssh_config)
            })
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    Ok((jumps, target))
}

/// `[user@]host[:port]`, the host in brackets when it is an IPv6 address.
fn parse_jump(jump: &str) -> Result<(Option<&str>, &str, Option<u16>), TunnelError> {
    let invalid = || {
        message(
            format!("The jump host “{jump}” is not valid"),
            "Write jump hosts as user@host:port, separated by commas. User and port are optional.",
            "invalid jump host",
        )
    };
    let (user, rest) = match jump.rsplit_once('@') {
        Some((user, rest)) => (Some(user), rest),
        None => (None, jump),
    };
    let (host, port) = if let Some(bracketed) = rest.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']').ok_or_else(invalid)?;
        (host, after.strip_prefix(':'))
    } else {
        match rest.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (rest, None),
        }
    };
    let port = port
        .map(|p| p.parse::<u16>().map_err(|_| invalid()))
        .transpose()?;
    if host.is_empty() {
        return Err(invalid());
    }
    Ok((user, host, port))
}

/// What the key check found, kept for the error message.
#[derive(Debug, Clone)]
enum HostKey {
    Unknown(PublicKey),
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
        let verdict =
            russh::keys::check_known_hosts_path(&self.host, self.port, key, &self.known_hosts);
        let rejected = match verdict {
            Ok(true) => return Ok(true),
            Err(russh::keys::Error::KeyChanged { .. }) => {
                HostKey::Changed(key.fingerprint(HashAlg::Sha256).to_string())
            }
            // No entry, or no readable known_hosts file.
            Ok(false) | Err(_) => HostKey::Unknown(key.clone()),
        };
        *self.rejected.lock().unwrap_or_else(|p| p.into_inner()) = Some(rejected);
        Ok(false)
    }
}

pub async fn open(spec: TunnelSpec) -> Result<Tunnel, TunnelError> {
    // Reads ~/.ssh/config: blocking IO, off the runtime's workers.
    let planned = spec.clone();
    let (jumps, target) = tokio::task::spawn_blocking(move || plan(&planned))
        .await
        .map_err(|e| message("Couldn't start the SSH tunnel", "Try again.", e))??;
    let mut sessions: Vec<Arc<Handle<Client>>> = Vec::new();
    for (index, hop) in jumps.iter().chain(std::iter::once(&target)).enumerate() {
        let is_jump = index < jumps.len();
        let via = sessions.last().cloned();
        let mut handle = handshake(via.as_deref(), hop, &spec.known_hosts).await?;
        let methods = if is_jump {
            jump_methods(hop, &spec)
        } else {
            target_methods(hop, &spec)?
        };
        authenticate(&mut handle, hop, &methods, is_jump).await?;
        sessions.push(Arc::new(handle));
    }
    let Some(handle) = sessions.pop() else {
        return Err(message(
            "Couldn't start the SSH tunnel",
            "No SSH server.",
            "no hop",
        ));
    };

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| message("Couldn't start the SSH tunnel", "No local port free.", e))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| message("Couldn't start the SSH tunnel", "No local port free.", e))?
        .port();
    let task = tokio::spawn(forward(
        listener,
        handle.clone(),
        spec.target_host.clone(),
        spec.target_port,
    ));
    tracing::info!(ssh = %target.address(), jumps = jumps.len(), local_port, "SSH tunnel open");
    Ok(Tunnel {
        local_port,
        handle,
        jumps: sessions,
        task,
    })
}

/// Connect to `hop` and check its key: directly, or through the jump host session `via`.
async fn handshake(
    via: Option<&Handle<Client>>,
    hop: &Hop,
    known_hosts: &Path,
) -> Result<Handle<Client>, TunnelError> {
    let address = hop.address();
    let rejected = Arc::new(Mutex::new(None));
    let handler = Client {
        host: hop.host.clone(),
        port: hop.port,
        known_hosts: known_hosts.to_owned(),
        rejected: rejected.clone(),
    };
    let config = Arc::new(client::Config {
        keepalive_interval: Some(KEEPALIVE),
        ..client::Config::default()
    });
    let connecting = async {
        match via {
            None => client::connect(config, (hop.host.as_str(), hop.port), handler).await,
            Some(jump) => {
                let channel = jump
                    .channel_open_direct_tcpip(
                        hop.host.clone(),
                        u32::from(hop.port),
                        "127.0.0.1",
                        0,
                    )
                    .await?;
                client::connect_stream(config, channel.into_stream(), handler).await
            }
        }
    };
    let err = match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
        Ok(Ok(handle)) => return Ok(handle),
        Ok(Err(err)) => err,
        Err(_) => {
            return Err(message(
                format!("Couldn't reach the SSH server {address}"),
                "No answer in time. Check the SSH host and port, and your network or VPN.",
                "ETIMEDOUT (SSH connect timed out)",
            ));
        }
    };
    let rejected = rejected.lock().unwrap_or_else(|p| p.into_inner()).clone();
    Err(match rejected {
        Some(HostKey::Unknown(key)) => {
            TunnelError::UnknownHost(Box::new(UnknownHost::new(&hop.host, hop.port, &key)))
        }
        Some(HostKey::Changed(fingerprint)) => message(
            format!("The SSH server {address} has a different key than before"),
            "This can mean someone is intercepting the connection. Ask the server's admin \
             whether the key changed; if it did, remove its old line from ~/.ssh/known_hosts.",
            format!("host key {fingerprint}"),
        ),
        Some(HostKey::Unsupported) => message(
            format!("The SSH server {address} uses a host certificate"),
            "Host certificates are not supported yet.",
            "host certificate",
        ),
        None if via.is_some() => message(
            format!("The jump host couldn't reach {address}"),
            "Check the host and port of the next server, as seen from the jump host.",
            err,
        ),
        None => message(
            format!("Couldn't connect to the SSH server {address}"),
            "Check the SSH host and port.",
            err,
        ),
    })
}

/// One way to sign in, tried in order.
enum Method {
    Password(String),
    KeyFile {
        path: PathBuf,
        passphrase: Option<String>,
    },
    Agent,
}

/// The SSH server: the way the connection names, then the alias's key files for the agent.
fn target_methods(hop: &Hop, spec: &TunnelSpec) -> Result<Vec<Method>, TunnelError> {
    let denied = |why: &str| {
        message(
            format!("The SSH server refused the sign-in for {}", hop.user),
            why,
            "SSH authentication failed",
        )
    };
    Ok(match spec.ssh.auth {
        SshAuth::Password => {
            let password = spec
                .secret
                .clone()
                .ok_or_else(|| denied("Enter the SSH password in the connection settings."))?;
            vec![Method::Password(password)]
        }
        SshAuth::Key => {
            let path = spec
                .ssh
                .key_path
                .as_deref()
                .filter(|p| !p.trim().is_empty())
                .map(expand_home)
                .or_else(|| hop.identities.first().cloned())
                .ok_or_else(|| denied("Choose a private key file in the connection settings."))?;
            vec![Method::KeyFile {
                path,
                passphrase: spec.secret.clone(),
            }]
        }
        SshAuth::Agent => std::iter::once(Method::Agent)
            .chain(hop.identities.iter().map(|path| Method::KeyFile {
                path: path.clone(),
                passphrase: None,
            }))
            .collect(),
    })
}

/// A jump host: the agent, the alias's key files, then the connection's own key file. Never the
/// SSH password, which belongs to the SSH server alone.
fn jump_methods(hop: &Hop, spec: &TunnelSpec) -> Vec<Method> {
    let mut methods = vec![Method::Agent];
    methods.extend(hop.identities.iter().map(|path| Method::KeyFile {
        path: path.clone(),
        passphrase: None,
    }));
    if spec.ssh.auth == SshAuth::Key
        && let Some(path) = spec
            .ssh
            .key_path
            .as_deref()
            .filter(|p| !p.trim().is_empty())
    {
        methods.push(Method::KeyFile {
            path: expand_home(path),
            passphrase: spec.secret.clone(),
        });
    }
    methods
}

async fn authenticate(
    handle: &mut Handle<Client>,
    hop: &Hop,
    methods: &[Method],
    is_jump: bool,
) -> Result<(), TunnelError> {
    let failed = |e: &dyn std::fmt::Display| {
        message(
            "SSH sign-in failed",
            "The connection broke while signing in.",
            e,
        )
    };
    // The last reason a method could not even be tried, shown when nothing worked.
    let mut unusable: Option<TunnelError> = None;
    for method in methods {
        let attempt = match method {
            Method::Password(password) => handle
                .authenticate_password(&hop.user, password)
                .await
                .map(|r| r.success())
                .map_err(|e| failed(&e)),
            Method::KeyFile { path, passphrase } => {
                let key = match russh::keys::load_secret_key(path, passphrase.as_deref()) {
                    Ok(key) => key,
                    Err(e) => {
                        unusable = Some(message(
                            format!("Couldn't read the SSH key {}", path.display()),
                            "Check the file path. If the key has a passphrase, enter it in the \
                             connection settings.",
                            e,
                        ));
                        continue;
                    }
                };
                let hash = handle
                    .best_supported_rsa_hash()
                    .await
                    .map_err(|e| failed(&e))?
                    .flatten();
                handle
                    .authenticate_publickey(
                        &hop.user,
                        PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                    )
                    .await
                    .map(|r| r.success())
                    .map_err(|e| failed(&e))
            }
            Method::Agent => match authenticate_with_agent(handle, &hop.user).await {
                Ok(success) => Ok(success),
                Err(err) => {
                    unusable = Some(err);
                    continue;
                }
            },
        };
        if attempt? {
            return Ok(());
        }
    }
    let address = hop.address();
    if is_jump {
        return Err(message(
            format!(
                "The jump host {address} refused the sign-in for {}",
                hop.user
            ),
            "Quarry signs in to jump hosts with ssh-agent, the key file of the host in \
             ~/.ssh/config, or the key file above. Add your key to ssh-agent (ssh-add), or \
             choose a key file.",
            "SSH authentication failed",
        ));
    }
    Err(unusable.unwrap_or_else(|| {
        message(
            format!(
                "The SSH server {address} refused the sign-in for {}",
                hop.user
            ),
            "Check the SSH user name and the password or key for that user.",
            "SSH authentication failed",
        )
    }))
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

pub(crate) fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), directories::BaseDirs::new()) {
        (Some(rest), Some(dirs)) => dirs.home_dir().join(rest),
        _ => Path::new(path).to_owned(),
    }
}

#[cfg(test)]
mod tests;
