use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::{
    Client, StatusCode, Url,
    header::{HeaderMap, RETRY_AFTER},
};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::config::Config;

/// Base delay for exponential backoff when the server gives no `Retry-After`.
const BACKOFF_BASE: Duration = Duration::from_millis(500);
/// Upper bound on any single backoff, including a server-supplied `Retry-After`.
const BACKOFF_MAX: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionMeta {
    pub created_at: DateTime<Utc>,
    pub yanked: bool,
    #[serde(default)]
    pub num: String,
}

#[derive(Debug, Deserialize)]
struct VersionResponse {
    version: VersionMeta,
}

#[derive(Debug, Deserialize)]
struct CrateResponse {
    versions: Vec<VersionMeta>,
}

#[derive(Clone)]
pub struct RegistryClient {
    http: Client,
    base: Url,
    retries: u32,
}

impl RegistryClient {
    pub fn new(config: &Config) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("cargo_cooldown_check")
            .build()?;
        let base = Url::parse(&config.registry_api).context("invalid registry API URL")?;
        Ok(Self {
            http,
            base,
            retries: config.http_retries,
        })
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: Url) -> Result<T> {
        let mut attempt = 0;
        loop {
            let response = self.http.get(url.clone()).send().await;

            let (retry_err, retry_after) = match response {
                Ok(resp) if is_transient_status(resp.status()) => {
                    log::warn!("Transient HTTP {} from {url}", resp.status());
                    let retry_after = retry_after_delay(resp.headers());
                    (resp.error_for_status().unwrap_err().into(), retry_after)
                }
                Ok(resp) => {
                    let status_resp = resp.error_for_status()?;
                    return Ok(status_resp.json::<T>().await?);
                }
                Err(err) => (err.into(), None),
            };

            attempt += 1;
            if attempt > self.retries {
                return Err(retry_err);
            }
            let backoff = backoff_delay(attempt, retry_after, &url);
            log::warn!(
                "Retrying {url} in {backoff:?} (attempt {attempt}/{})",
                self.retries
            );
            sleep(backoff).await;
        }
    }

    pub async fn fetch_version(&self, name: &str, version: &str) -> Result<VersionMeta> {
        let url = self
            .base
            .join(&format!("crates/{name}/{version}"))
            .with_context(|| format!("failed to build version URL for {name}:{version}"))?;
        let resp: VersionResponse = self.get_json(url).await?;
        Ok(resp.version)
    }

    pub async fn list_versions(&self, name: &str) -> Result<Vec<VersionMeta>> {
        let url = self
            .base
            .join(&format!("crates/{name}"))
            .with_context(|| format!("failed to build crate URL for {name}"))?;
        let resp: CrateResponse = self.get_json(url).await?;
        Ok(resp.versions)
    }
}

fn is_transient_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE
}

/// crates.io returns `Retry-After: <seconds>` on 429; honor it (HTTP-date form
/// is not used by crates.io, so we only parse the delta-seconds form).
fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
    let secs = headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(secs))
}

/// Delay before the next retry: the server's `Retry-After` when present, else
/// exponential backoff. A per-URL jitter desynchronizes the concurrent fetchers
/// so a burst that all gets rate-limited doesn't retry into the same window.
fn backoff_delay(attempt: u32, retry_after: Option<Duration>, url: &Url) -> Duration {
    let base = retry_after
        .unwrap_or_else(|| BACKOFF_BASE.saturating_mul(2u32.saturating_pow(attempt - 1)));
    base.min(BACKOFF_MAX) + jitter(url, attempt)
}

/// Deterministic 0-250ms jitter keyed on the URL, so concurrent retriers (each
/// a distinct crate URL) stagger without needing a random source.
fn jitter(url: &Url, attempt: u32) -> Duration {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.as_str().hash(&mut hasher);
    attempt.hash(&mut hasher);
    Duration::from_millis(hasher.finish() % 250)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_parses_delta_seconds() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, "30".parse().unwrap());
        assert_eq!(retry_after_delay(&headers), Some(Duration::from_secs(30)));
    }

    #[test]
    fn retry_after_absent_or_unparseable_is_none() {
        assert_eq!(retry_after_delay(&HeaderMap::new()), None);

        let mut headers = HeaderMap::new();
        headers.insert(
            RETRY_AFTER,
            "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after_delay(&headers), None);
    }

    #[test]
    fn backoff_grows_exponentially_without_retry_after() {
        let url = Url::parse("https://crates.io/api/v1/crates/serde/1.0.0").unwrap();
        let base = |attempt| backoff_delay(attempt, None, &url) - jitter(&url, attempt);
        assert_eq!(base(1), BACKOFF_BASE);
        assert_eq!(base(2), BACKOFF_BASE * 2);
        assert_eq!(base(3), BACKOFF_BASE * 4);
    }

    #[test]
    fn backoff_honors_and_caps_retry_after() {
        let url = Url::parse("https://crates.io/api/v1/crates/serde/1.0.0").unwrap();
        let with = |secs| backoff_delay(1, Some(Duration::from_secs(secs)), &url) - jitter(&url, 1);
        assert_eq!(with(30), Duration::from_secs(30));
        assert_eq!(with(600), BACKOFF_MAX);
    }
}
