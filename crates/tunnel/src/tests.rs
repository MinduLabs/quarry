use std::sync::atomic::{AtomicUsize, Ordering};

use russh::keys::PrivateKey;
use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::server::{Auth, ChannelOpenHandle, Msg, Session};
use russh::{Channel, MethodKind, MethodSet};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

/// A key from a fixed seed, for these tests only.
fn key(seed: u8) -> PrivateKey {
    PrivateKey::from(Ed25519Keypair::from_seed(&[seed; 32]))
}

/// An SSH server on a local port that forwards `direct-tcpip` channels, and counts the
/// passwords it is sent.
#[derive(Clone)]
struct TestServer {
    accepts_password: bool,
    accepts_keys: bool,
    passwords: Arc<AtomicUsize>,
}

impl russh::server::Handler for TestServer {
    type Error = russh::Error;

    async fn auth_password(&mut self, _: &str, _: &str) -> Result<Auth, Self::Error> {
        self.passwords.fetch_add(1, Ordering::SeqCst);
        Ok(self.verdict(self.accepts_password))
    }

    async fn auth_publickey_offered(
        &mut self,
        _: &str,
        _: russh::server::PublicKeyOffer<'_>,
    ) -> Result<Auth, Self::Error> {
        Ok(self.verdict(self.accepts_keys))
    }

    async fn auth_publickey(
        &mut self,
        _: &str,
        _: russh::server::VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        Ok(self.verdict(self.accepts_keys))
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        let address = format!("{host}:{port}");
        reply.accept().await;
        tokio::spawn(async move {
            if let Ok(mut socket) = tokio::net::TcpStream::connect(address).await {
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
            }
        });
        Ok(())
    }
}

impl TestServer {
    fn new(accepts_password: bool, accepts_keys: bool) -> Self {
        Self {
            accepts_password,
            accepts_keys,
            passwords: Arc::default(),
        }
    }

    fn verdict(&self, accepted: bool) -> Auth {
        if accepted {
            Auth::Accept
        } else {
            Auth::Reject {
                proceed_with_methods: Some(MethodSet::from(
                    &[MethodKind::Password, MethodKind::PublicKey][..],
                )),
                partial_success: false,
            }
        }
    }

    /// Serve on a free port with host key `seed`; the port and the `known_hosts` line.
    async fn start(&self, seed: u8) -> (u16, String) {
        let host_key = key(seed);
        let public = host_key.public_key().to_openssh().unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = Arc::new(russh::server::Config {
            keys: vec![host_key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let handler = self.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (config, handler) = (config.clone(), handler.clone());
                tokio::spawn(async move {
                    if let Ok(session) = russh::server::run_stream(config, stream, handler).await {
                        let _ = session.await;
                    }
                });
            }
        });
        (port, format!("[127.0.0.1]:{port} {public}\n"))
    }
}

/// A TCP server that sends back what it reads: the "database" behind the tunnel.
async fn echo_server() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut read, mut write) = socket.split();
                let _ = tokio::io::copy(&mut read, &mut write).await;
            });
        }
    });
    port
}

async fn assert_echoes(tunnel: &Tunnel) {
    let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", tunnel.local_port()))
        .await
        .unwrap();
    socket.write_all(b"ping").await.unwrap();
    let mut reply = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), socket.read_exact(&mut reply))
        .await
        .expect("no answer through the tunnel")
        .unwrap();
    assert_eq!(&reply, b"ping");
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "quarry-tunnel-{}-{}",
        std::process::id(),
        uuid_like()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn uuid_like() -> u64 {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    NEXT.fetch_add(1, Ordering::SeqCst) as u64
}

fn spec(port: u16, auth: SshAuth, known_hosts: PathBuf, echo: u16) -> TunnelSpec {
    TunnelSpec {
        ssh: SshConfig {
            host: "127.0.0.1".into(),
            port,
            user: "me".into(),
            auth,
            key_path: None,
            jump: None,
        },
        secret: Some("secret".into()),
        target_host: "127.0.0.1".into(),
        target_port: echo,
        known_hosts,
        ssh_config: None,
    }
}

#[tokio::test]
async fn forwards_to_the_database() {
    let server = TestServer::new(true, false);
    let (port, line) = server.start(1).await;
    let dir = temp_dir();
    std::fs::write(dir.join("known_hosts"), line).unwrap();
    let echo = echo_server().await;
    let tunnel = open(spec(port, SshAuth::Password, dir.join("known_hosts"), echo))
        .await
        .unwrap();
    assert_echoes(&tunnel).await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn an_unknown_server_can_be_trusted_after_checking_its_fingerprint() {
    let server = TestServer::new(true, false);
    let (port, _) = server.start(2).await;
    let dir = temp_dir();
    let known_hosts = dir.join("known_hosts");
    let echo = echo_server().await;
    let Err(TunnelError::UnknownHost(unknown)) =
        open(spec(port, SshAuth::Password, known_hosts.clone(), echo)).await
    else {
        panic!("an unknown server was not refused")
    };
    assert_eq!(
        unknown.fingerprint,
        key(2).public_key().fingerprint(HashAlg::Sha256).to_string()
    );
    assert_eq!((unknown.host.as_str(), unknown.port), ("127.0.0.1", port));
    // The password is not sent before the server is trusted.
    assert_eq!(server.passwords.load(Ordering::SeqCst), 0);

    unknown.trust(&known_hosts).unwrap();
    let tunnel = open(spec(port, SshAuth::Password, known_hosts, echo))
        .await
        .unwrap();
    assert_echoes(&tunnel).await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_changed_key_is_refused_and_never_offered_for_trust() {
    let server = TestServer::new(true, false);
    let (port, _) = server.start(3).await;
    let dir = temp_dir();
    let known_hosts = dir.join("known_hosts");
    let old = key(30).public_key().to_openssh().unwrap();
    std::fs::write(&known_hosts, format!("[127.0.0.1]:{port} {old}\n")).unwrap();
    let echo = echo_server().await;
    let err = open(spec(port, SshAuth::Password, known_hosts, echo))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, TunnelError::Message(m) if m.contains("different key")),
        "{err}"
    );
    assert_eq!(server.passwords.load(Ordering::SeqCst), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn goes_through_a_jump_host_with_the_key_file() {
    let jump = TestServer::new(false, true);
    let target = TestServer::new(false, true);
    let (jump_port, jump_line) = jump.start(4).await;
    let (port, line) = target.start(5).await;
    let dir = temp_dir();
    let known_hosts = dir.join("known_hosts");
    std::fs::write(&known_hosts, format!("{jump_line}{line}")).unwrap();
    let key_file = dir.join("id_ed25519");
    std::fs::write(
        &key_file,
        key(9)
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    let echo = echo_server().await;
    let mut spec = spec(port, SshAuth::Key, known_hosts, echo);
    spec.ssh.key_path = Some(key_file.display().to_string());
    spec.ssh.jump = Some(format!("hop@127.0.0.1:{jump_port}"));
    spec.secret = None;
    let tunnel = open(spec).await.unwrap();
    assert_echoes(&tunnel).await;
    assert!(tunnel.is_alive());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn the_ssh_password_is_never_sent_to_a_jump_host() {
    // Takes passwords only: the tunnel must fail rather than hand it the SSH password.
    let jump = TestServer::new(true, false);
    let target = TestServer::new(true, false);
    let (jump_port, jump_line) = jump.start(6).await;
    let (port, line) = target.start(7).await;
    let dir = temp_dir();
    let known_hosts = dir.join("known_hosts");
    std::fs::write(&known_hosts, format!("{jump_line}{line}")).unwrap();
    let echo = echo_server().await;
    let mut spec = spec(port, SshAuth::Password, known_hosts, echo);
    spec.ssh.jump = Some(format!("127.0.0.1:{jump_port}"));
    let err = open(spec).await.unwrap_err();
    assert!(err.to_string().contains("jump host"), "{err}");
    assert_eq!(jump.passwords.load(Ordering::SeqCst), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_tunnel_reports_when_its_ssh_connection_ends() {
    let host_key = key(8);
    let public = host_key.public_key().to_openssh().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let dir = temp_dir();
    let known_hosts = dir.join("known_hosts");
    std::fs::write(&known_hosts, format!("[127.0.0.1]:{port} {public}")).unwrap();
    let config = Arc::new(russh::server::Config {
        keys: vec![host_key],
        auth_rejection_time: Duration::ZERO,
        ..Default::default()
    });
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        // The session runs on its own task; the handle hangs it up.
        russh::server::run_stream(config, stream, TestServer::new(true, false))
            .await
            .unwrap()
            .handle()
    });
    let tunnel = open(spec(port, SshAuth::Password, known_hosts, 9))
        .await
        .unwrap();
    assert!(tunnel.is_alive());

    server
        .await
        .unwrap()
        .disconnect(
            russh::Disconnect::ByApplication,
            "restart".into(),
            "en".into(),
        )
        .await
        .unwrap();
    let closed = async {
        while tunnel.is_alive() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), closed)
        .await
        .expect("the tunnel still says it is alive after the server hung up");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn jump_hosts_are_written_as_proxy_jump() {
    assert_eq!(
        parse_jump("ops@bastion:2222").unwrap(),
        (Some("ops"), "bastion", Some(2222))
    );
    assert_eq!(parse_jump("bastion").unwrap(), (None, "bastion", None));
    assert_eq!(
        parse_jump("me@[2001:db8::1]:22").unwrap(),
        (Some("me"), "2001:db8::1", Some(22))
    );
    assert!(parse_jump("bastion:port").is_err());
    assert!(parse_jump("me@").is_err());
}

#[test]
fn an_alias_fills_what_the_connection_leaves_empty() {
    let dir = temp_dir();
    let config = dir.join("config");
    std::fs::write(
        &config,
        "Host prod\n  HostName 10.0.0.5\n  Port 2200\n  User deploy\n  ProxyJump ops@bastion\n\
         Host bastion\n  HostName 192.0.2.10\n  IdentityFile /keys/bastion\n",
    )
    .unwrap();
    let mut spec = spec(22, SshAuth::Agent, dir.join("known_hosts"), 3306);
    spec.ssh.host = "prod".into();
    spec.ssh.user = String::new();
    spec.ssh_config = Some(config);
    let (jumps, target) = plan(&spec).unwrap();
    assert_eq!(
        target,
        Hop {
            host: "10.0.0.5".into(),
            port: 2200,
            user: "deploy".into(),
            identities: Vec::new(),
        }
    );
    assert_eq!(
        jumps,
        vec![Hop {
            host: "192.0.2.10".into(),
            port: 22,
            user: "ops".into(),
            identities: vec![PathBuf::from("/keys/bastion")],
        }]
    );

    // What the connection names wins; "none" turns the alias's jump off.
    spec.ssh.port = 2022;
    spec.ssh.user = "me".into();
    spec.ssh.jump = Some("none".into());
    let (jumps, target) = plan(&spec).unwrap();
    assert_eq!((target.port, target.user.as_str()), (2022, "me"));
    assert!(jumps.is_empty());
    spec.ssh.jump = None;
    std::fs::write(
        spec.ssh_config.as_ref().unwrap(),
        "Host prod\n  ProxyJump none\n",
    )
    .unwrap();
    assert!(plan(&spec).unwrap().0.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

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
    let spec = spec(22, SshAuth::Password, PathBuf::from("/dev/null"), 3306);
    assert!(!format!("{spec:?}").contains("secret\""));
    assert!(format!("{spec:?}").contains("***"));
}
