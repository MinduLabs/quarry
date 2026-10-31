//! Whether a newer Quarry is released. Quarry does not install it: the client links to the
//! release page, where the user downloads the package they use.

use serde::Deserialize;

use crate::engine::Engine;
use crate::{Event, VERSION};

/// The newest release that is not a draft or a prerelease.
const LATEST_URL: &str = "https://api.github.com/repos/MinduLabs/quarry/releases/latest";

/// The only pages the client is sent to.
const RELEASES_PAGE: &str = "https://github.com/MinduLabs/quarry/releases/";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Release {
    tag_name: String,
    html_url: String,
}

impl Engine {
    pub(crate) async fn check_for_update(&self) {
        let release = self
            .update
            .get_or_init(|| async {
                let latest = self
                    .update_url
                    .clone()
                    .unwrap_or_else(|| LATEST_URL.to_owned());
                // Nothing to show the user: without the check, Quarry works the same.
                newer_release(&latest).await.unwrap_or_else(|error| {
                    tracing::warn!(%error, "update check failed");
                    None
                })
            })
            .await;
        if let Some(release) = release {
            self.emit(Event::UpdateAvailable {
                version: release.tag_name.trim_start_matches('v').to_owned(),
                url: release.html_url.clone(),
            });
        }
    }
}

/// The release of a version newer than this one, if there is one.
async fn newer_release(latest: &str) -> Result<Option<Release>, String> {
    let body = crate::http::client()?
        .get(latest)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|err| err.to_string())?
        .bytes()
        .await
        .map_err(|err| err.to_string())?;
    let release: Release = serde_json::from_slice(&body).map_err(|err| err.to_string())?;
    if !release.html_url.starts_with(RELEASES_PAGE) {
        return Err(format!("unexpected release page {}", release.html_url));
    }
    Ok(is_newer(&release.tag_name, VERSION).then_some(release))
}

/// `tag` (`v1.2.3`) is a later version than `current`. A tag that is not a version never is.
fn is_newer(tag: &str, current: &str) -> bool {
    let parse = |v: &str| semver::Version::parse(v.strip_prefix('v').unwrap_or(v)).ok();
    match (parse(tag), parse(current)) {
        (Some(tag), Some(current)) => tag > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{is_newer, newer_release};
    use crate::{Command, EngineHandle, EngineOptions, Event};

    /// Answers every request with `status` and `body`; counts the requests.
    async fn serve(status: &'static str, body: String) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let counted = requests.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                let body = body.clone();
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
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("{base}/latest"), requests)
    }

    fn release(tag: &str, page: &str) -> String {
        serde_json::json!({ "tag_name": tag, "html_url": page, "prerelease": false }).to_string()
    }

    const PAGE: &str = "https://github.com/MinduLabs/quarry/releases/tag/v99.0.0";

    #[test]
    fn only_a_later_version_is_newer() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(!is_newer("v1.0.0", "not a version"));
    }

    #[tokio::test]
    async fn a_newer_release_gives_its_page() {
        let (url, _) = serve("200 OK", release("v99.0.0", PAGE)).await;
        let release = newer_release(&url).await.unwrap().unwrap();
        assert_eq!(release.html_url, PAGE);
    }

    #[tokio::test]
    async fn the_running_version_is_not_an_update() {
        let tag = format!("v{}", crate::VERSION);
        let page = format!("https://github.com/MinduLabs/quarry/releases/tag/{tag}");
        let (url, _) = serve("200 OK", release(&tag, &page)).await;
        assert_eq!(newer_release(&url).await, Ok(None));
    }

    #[tokio::test]
    async fn a_page_outside_the_releases_is_refused() {
        let (url, _) = serve("200 OK", release("v99.0.0", "https://example.com/quarry")).await;
        assert!(newer_release(&url).await.is_err());
    }

    #[tokio::test]
    async fn an_http_error_or_a_bad_answer_fails() {
        let (missing, _) = serve("404 Not Found", "{}".to_owned()).await;
        assert!(newer_release(&missing).await.is_err());
        let (limited, _) = serve("403 Forbidden", "{}".to_owned()).await;
        assert!(newer_release(&limited).await.is_err());
        let (garbled, _) = serve("200 OK", "not json".to_owned()).await;
        assert!(newer_release(&garbled).await.is_err());
    }

    #[tokio::test]
    async fn github_is_asked_once_per_run() {
        let (url, requests) = serve("200 OK", release("v99.0.0", PAGE)).await;
        let dir = crate::test_support::temp_dir();
        let (engine, events) = EngineHandle::spawn_with(EngineOptions {
            update_url: Some(url),
            ..crate::test_support::options(&dir)
        })
        .unwrap();
        for _ in 0..2 {
            engine.send(Command::CheckForUpdate).unwrap();
            let event = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if let Event::UpdateAvailable { version, url } = events.recv().await.unwrap() {
                        return (version, url);
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(event, ("99.0.0".to_owned(), PAGE.to_owned()));
        }
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }
}
