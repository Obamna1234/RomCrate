//! A locked-down HTTP client.
//!
//! * HTTPS only, modern TLS via rustls (no OpenSSL dependency).
//! * Only talks to an explicit allow-list of hosts — including after redirects.
//! * No cookies, no credentials, no tracking headers; a plain User-Agent.
//! * Response bodies are size-capped while streaming, so a hostile or broken
//!   server can't fill the disk or memory.

use futures::StreamExt;
use std::time::Duration;
use thiserror::Error;
use url::Url;

/// The only hosts ShelfArt will ever contact.
pub const ALLOWED_HOSTS: &[&str] = &[
    "thumbnails.libretro.com",
    "raw.githubusercontent.com",
    "api.github.com",
];
/// ScreenScraper serves its API and media from several sub-domains.
pub const ALLOWED_DOMAIN_SUFFIXES: &[&str] = &[".screenscraper.fr"];

/// Largest image we accept (libretro art is typically 50 KB – 2 MB).
pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;
/// Largest video we accept.
pub const MAX_VIDEO_BYTES: usize = 150 * 1024 * 1024;
/// Largest manual we accept.
pub const MAX_MANUAL_BYTES: usize = 200 * 1024 * 1024;
/// Largest index / listing we accept.
pub const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum NetError {
    #[error("host not allowed: {0}")]
    HostNotAllowed(String),
    #[error("not found")]
    NotFound,
    #[error("rate limited by server, try again later")]
    RateLimited,
    #[error("response too large (>{0} bytes)")]
    TooLarge(usize),
    #[error("HTTP status {0}")]
    Status(u16),
    #[error("network error: {0}")]
    Transport(String),
}

pub fn host_allowed(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .map(|h| {
                ALLOWED_HOSTS.contains(&h)
                    || h == "screenscraper.fr"
                    || ALLOWED_DOMAIN_SUFFIXES.iter().any(|s| h.ends_with(s))
            })
            .unwrap_or(false)
}

#[derive(Clone)]
pub struct SafeClient {
    client: reqwest::Client,
}

impl SafeClient {
    pub fn new() -> anyhow::Result<Self> {
        let policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else if host_allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        });
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(policy)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(90))
            .user_agent(concat!(
                "ShelfArt/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/Obamna1234/ShelfArt)"
            ))
            .build()?;
        Ok(Self { client })
    }

    /// GET a URL and return the body, capped at `max` bytes. Retries transient failures.
    pub async fn get(&self, url: &Url, max: usize) -> Result<Vec<u8>, NetError> {
        if !host_allowed(url) {
            return Err(NetError::HostNotAllowed(
                url.host_str().unwrap_or("?").to_string(),
            ));
        }
        let mut last = NetError::Transport("no attempt".into());
        for attempt in 0..3u32 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(600 * 2u64.pow(attempt))).await;
            }
            match self.get_once(url, max).await {
                Ok(b) => return Ok(b),
                Err(e @ (NetError::Transport(_) | NetError::Status(500..=599))) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    async fn get_once(&self, url: &Url, max: usize) -> Result<Vec<u8>, NetError> {
        let resp = self
            .client
            .get(url.clone())
            .header("Accept", "*/*")
            .send()
            .await
            .map_err(transport)?;
        if !host_allowed(resp.url()) {
            return Err(NetError::HostNotAllowed(
                resp.url().host_str().unwrap_or("?").to_string(),
            ));
        }
        let status = resp.status().as_u16();
        match status {
            200 => {}
            404 | 410 => return Err(NetError::NotFound),
            429 => return Err(NetError::RateLimited),
            403 if url.host_str() == Some("api.github.com") => return Err(NetError::RateLimited),
            s => return Err(NetError::Status(s)),
        }
        if let Some(len) = resp.content_length() {
            if len as usize > max {
                return Err(NetError::TooLarge(max));
            }
        }
        let mut body = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(transport)?;
            if body.len() + chunk.len() > max {
                return Err(NetError::TooLarge(max));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

/// Network errors without the request URL: ScreenScraper URLs carry the
/// app's developer password (and optionally the user's), which must never
/// end up in logs, the UI or a saved report.
fn transport(e: reqwest::Error) -> NetError {
    let e = e.without_url();
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    NetError::Transport(redact(&msg))
}

/// Remove anything that looks like a credential from a message.
pub fn redact(msg: &str) -> String {
    let mut out = msg.to_string();
    for key in ["devpassword=", "sspassword=", "devid=", "ssid="] {
        let mut from = 0;
        while let Some(rel) = out[from..].find(key) {
            let start = from + rel + key.len();
            let end = out[start..]
                .find(['&', ' ', ')', '"', '\''])
                .map(|e| start + e)
                .unwrap_or(out.len());
            out.replace_range(start..end, "[hidden]");
            from = start + "[hidden]".len();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_never_leak() {
        let m = redact("error sending request for url (https://api.screenscraper.fr/api2/x?devid=abc&devpassword=SECRET&ssid=me&sspassword=hunter2)");
        assert!(
            !m.contains("SECRET")
                && !m.contains("hunter2")
                && !m.contains("abc")
                && !m.contains("ssid=me"),
            "{m}"
        );
    }

    #[tokio::test]
    async fn transport_errors_hide_urls() {
        let c = SafeClient::new().unwrap();
        // An allowed host on a port nothing listens on: fails fast, offline.
        let url = Url::parse("https://api.screenscraper.fr:1/api2/x?devpassword=SECRET").unwrap();
        if let Err(e) = c.get_once(&url, 10).await {
            assert!(!e.to_string().contains("SECRET"), "{e}");
        }
    }

    #[test]
    fn allow_list() {
        assert!(host_allowed(
            &Url::parse("https://raw.githubusercontent.com/a/b").unwrap()
        ));
        assert!(!host_allowed(
            &Url::parse("http://raw.githubusercontent.com/a/b").unwrap()
        ));
        assert!(!host_allowed(
            &Url::parse("https://evil.example.com/a").unwrap()
        ));
        assert!(!host_allowed(
            &Url::parse("https://raw.githubusercontent.com.evil.com/a").unwrap()
        ));
        assert!(host_allowed(
            &Url::parse("https://neoclone.screenscraper.fr/api2/x").unwrap()
        ));
        assert!(!host_allowed(
            &Url::parse("https://evilscreenscraper.fr/x").unwrap()
        ));
    }
}
