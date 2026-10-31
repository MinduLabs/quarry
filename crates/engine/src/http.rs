//! The HTTP client for everything Quarry fetches from its releases.

use std::sync::Arc;
use std::time::Duration;

pub(crate) fn client() -> Result<reqwest::Client, String> {
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
