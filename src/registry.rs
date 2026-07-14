use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode, Url};
use tame_index::{
    IndexKrate, IndexVersion, KrateName,
    index::{FileLock, IndexLocation, IndexUrl, SparseIndex},
};
use tokio::time::sleep;

/// Bounded retry for spurious network failures, mirroring cargo's `net.retry`.
const HTTP_ATTEMPTS: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct VersionMeta {
    pub created_at: DateTime<Utc>,
    pub yanked: bool,
    pub num: String,
}

/// Publication metadata source: the crates.io sparse index, whose entries
/// carry a `pubtime` field. Reads cargo's own on-disk index cache first —
/// already populated by the `cargo metadata` call made at startup — and
/// falls back to `index.crates.io`, the CDN host cargo bulk-fetches from,
/// which unlike the `crates.io/api` host has no request-rate budget.
pub struct RegistryClient {
    http: Client,
    index: SparseIndex,
    lock: FileLock,
}

impl RegistryClient {
    pub fn new() -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(
                "cargo-cooldown-check (https://github.com/NomicFoundation/cargo-cooldown-check)",
            )
            .build()?;
        let index = SparseIndex::new(IndexLocation::new(IndexUrl::CratesIoSparse))
            .context("failed to locate the crates.io sparse index")?;
        Ok(Self {
            http,
            index,
            lock: FileLock::unlocked(),
        })
    }

    pub async fn fetch_version(&self, name: &str, version: &str) -> Result<VersionMeta> {
        // A local entry may predate crates.io's `pubtime` backfill; falling
        // through to a remote fetch returns the timestamped entry.
        if let Some(meta) = self
            .local_krate(name)
            .as_ref()
            .and_then(|krate| find_version(krate, version))
            .and_then(|indexed| version_meta(indexed).ok())
        {
            return Ok(meta);
        }
        let krate = self.fetch_krate(name).await?;
        let indexed = find_version(&krate, version)
            .with_context(|| format!("{name}@{version} not found in the crates.io index"))?;
        version_meta(indexed)
    }

    /// The local index file is always sufficient for downgrade candidates:
    /// it contains the lockfile's own version, hence every older one too.
    /// Only `yanked` flags can be stale — at worst since the failing version
    /// was published — and a wrongly suggested version fails loudly in
    /// `cargo update`.
    pub async fn list_versions(&self, name: &str) -> Result<Vec<VersionMeta>> {
        // A local file predating crates.io's `pubtime` backfill yields no
        // usable versions; the remote copy is timestamped.
        if let Some(krate) = self.local_krate(name) {
            let versions = version_metas(&krate);
            if !versions.is_empty() {
                return Ok(versions);
            }
        }
        let krate = self.fetch_krate(name).await?;
        Ok(version_metas(&krate))
    }

    fn local_krate(&self, name: &str) -> Option<IndexKrate> {
        let krate_name = KrateName::crates_io(name).ok()?;
        self.index.cached_krate(krate_name, &self.lock).ok()?
    }

    async fn fetch_krate(&self, name: &str) -> Result<IndexKrate> {
        let krate_name = KrateName::crates_io(name)?;
        let url = Url::parse(&self.index.crate_url(krate_name))
            .with_context(|| format!("failed to build index URL for {name}"))?;
        let body = self.get_with_retry(url).await?;
        IndexKrate::from_slice(&body)
            .with_context(|| format!("failed to parse index entry for {name}"))
    }

    async fn get_with_retry(&self, url: Url) -> Result<Vec<u8>> {
        let mut attempt = 1;
        loop {
            let err = match self.http.get(url.clone()).send().await {
                Ok(resp) if !is_transient_status(resp.status()) => {
                    return Ok(resp.error_for_status()?.bytes().await?.to_vec());
                }
                Ok(resp) => resp.error_for_status().unwrap_err(),
                Err(err) => err,
            };
            if attempt == HTTP_ATTEMPTS {
                return Err(err.into());
            }
            log::warn!("Retrying {url} after transient failure: {err}");
            attempt += 1;
            sleep(RETRY_DELAY).await;
        }
    }
}

// Silently skip versions without a valid pubtime - at worst we omit a
// candidate from the downgrade suggestions.
fn version_metas(krate: &IndexKrate) -> Vec<VersionMeta> {
    krate
        .versions
        .iter()
        .filter_map(|indexed| version_meta(indexed).ok())
        .collect()
}

fn find_version<'krate>(krate: &'krate IndexKrate, version: &str) -> Option<&'krate IndexVersion> {
    krate
        .versions
        .iter()
        .find(|indexed| indexed.version.as_str() == version)
}

fn version_meta(indexed: &IndexVersion) -> Result<VersionMeta> {
    let Some(pubtime) = &indexed.pubtime else {
        bail!(
            "index entry for {}@{} has no publish time",
            indexed.name,
            indexed.version
        );
    };
    let created_at = DateTime::parse_from_rfc3339(pubtime)
        .with_context(|| {
            format!(
                "invalid publish time {pubtime:?} for {}@{}",
                indexed.name, indexed.version
            )
        })?
        .with_timezone(&Utc);
    Ok(VersionMeta {
        created_at,
        yanked: indexed.yanked,
        num: indexed.version.to_string(),
    })
}

fn is_transient_status(status: StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn indexed_version(pubtime: Option<&str>) -> IndexVersion {
        let mut indexed = IndexVersion::fake("serde", "1.0.0");
        indexed.pubtime = pubtime.map(Into::into);
        indexed
    }

    #[test]
    fn version_meta_parses_pubtime() {
        let meta = version_meta(&indexed_version(Some("2026-06-25T20:43:34Z"))).unwrap();
        assert_eq!(
            meta.created_at,
            Utc.with_ymd_and_hms(2026, 6, 25, 20, 43, 34).unwrap()
        );
        assert_eq!(meta.num, "1.0.0");
        assert!(!meta.yanked);
    }

    #[test]
    fn version_meta_errors_without_pubtime() {
        let err = version_meta(&indexed_version(None)).unwrap_err();
        assert!(format!("{err:#}").contains("no publish time"));
    }

    #[test]
    fn version_meta_errors_on_invalid_pubtime() {
        let err = version_meta(&indexed_version(Some("yesterday"))).unwrap_err();
        assert!(format!("{err:#}").contains("invalid publish time"));
    }
}
