use anyhow::Context;
use chrono::{DateTime, Utc};
use semver::{Version, VersionReq};

use crate::{
    registry::{RegistryClient, VersionMeta},
    types::CooldownFailure,
};

pub struct Resolver {
    client: RegistryClient,
    /// A single reference point in time for all cooldown comparisons,
    /// ensuring consistency across the entire check.
    reference_time: DateTime<Utc>,
}

impl Resolver {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: RegistryClient::new()?,
            reference_time: Utc::now(),
        })
    }

    pub async fn find_version_candidates(
        &self,
        cooldown_failure: &CooldownFailure,
        requirements: &[VersionReq],
    ) -> anyhow::Result<Vec<Version>> {
        let current_version =
            Version::parse(&cooldown_failure.current_version).context(format!(
                "Could not parse {}@{} version",
                cooldown_failure.name, cooldown_failure.current_version
            ))?;
        let mut candidate_list = self.client.list_versions(&cooldown_failure.name).await?;
        candidate_list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        let cutoff = self.reference_time
            - chrono::Duration::minutes(cooldown_failure.age_threshold_minutes as i64);

        Ok(candidate_versions(
            candidate_list,
            &current_version,
            cutoff,
            requirements,
        ))
    }

    pub async fn fetch_version_age(&self, name: &str, version: &str) -> anyhow::Result<u64> {
        let meta = self.client.fetch_version(name, version).await?;
        Ok((self.reference_time - meta.created_at)
            .num_minutes()
            .try_into()
            .unwrap_or(0))
    }
}

fn candidate_versions(
    candidate_list: Vec<VersionMeta>,
    current_version: &Version,
    cutoff: DateTime<Utc>,
    requirements: &[VersionReq],
) -> Vec<Version> {
    candidate_list
        .into_iter()
        .filter(|meta| !meta.yanked)
        .filter(|meta| meta.created_at <= cutoff)
        // Silently skip unparseable versions - at worst we omit a candidate from the downgrade suggestions.
        .filter_map(|meta| Version::parse(&meta.num).ok())
        .filter(|version| {
            *version < *current_version && satisfies_requirements(version, requirements)
        })
        .collect()
}

fn satisfies_requirements(version: &Version, requirements: &[VersionReq]) -> bool {
    if requirements.is_empty() {
        return true;
    }
    requirements.iter().all(|req| req.matches(version))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn version_meta(num: &str, created_at: DateTime<Utc>, yanked: bool) -> VersionMeta {
        VersionMeta {
            num: num.into(),
            created_at,
            yanked,
        }
    }

    fn tokio_versions() -> Vec<VersionMeta> {
        let old = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        ["1.44.0", "1.43.4", "1.43.3", "1.43.0", "1.42.1", "1.41.0"]
            .map(|num| version_meta(num, old, false))
            .into_iter()
            .collect()
    }

    fn current_version() -> Version {
        Version::parse("1.43.4").unwrap()
    }

    fn far_future_cutoff() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap()
    }

    #[test]
    fn candidate_versions_returns_versions_satisfying_all_requirements() {
        let requirements = vec![
            VersionReq::parse("^1").unwrap(),
            VersionReq::parse("^1.42").unwrap(),
            VersionReq::parse("^1.43").unwrap(),
        ];

        let candidates = candidate_versions(
            tokio_versions(),
            &current_version(),
            far_future_cutoff(),
            &requirements,
        );

        let versions: Vec<String> = candidates.iter().map(ToString::to_string).collect();
        assert_eq!(versions, vec!["1.43.3", "1.43.0"]);
    }

    #[test]
    fn candidate_versions_excludes_yanked_versions() {
        let old = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let versions = vec![
            version_meta("1.43.4", old, false),
            version_meta("1.43.3", old, true),
            version_meta("1.43.0", old, false),
        ];
        let requirements = vec![VersionReq::parse("^1.43").unwrap()];

        let candidates = candidate_versions(
            versions,
            &current_version(),
            far_future_cutoff(),
            &requirements,
        );

        let versions: Vec<String> = candidates.iter().map(ToString::to_string).collect();
        assert_eq!(versions, vec!["1.43.0"]);
    }

    #[test]
    fn candidate_versions_excludes_versions_within_cooldown_period() {
        let old = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let fresh = Utc::now();
        let versions = vec![
            version_meta("1.43.4", fresh, false),
            version_meta("1.43.3", fresh, false),
            version_meta("1.43.0", old, false),
        ];
        let cutoff = Utc::now() - chrono::Duration::minutes(10080);
        let requirements = vec![VersionReq::parse("^1.43").unwrap()];

        let candidates = candidate_versions(versions, &current_version(), cutoff, &requirements);

        let versions: Vec<String> = candidates.iter().map(ToString::to_string).collect();
        assert_eq!(versions, vec!["1.43.0"]);
    }

    #[test]
    fn candidate_versions_returns_empty_when_no_older_version_satisfies_requirements() {
        let requirements = vec![VersionReq::parse("^1.43.4").unwrap()];

        let candidates = candidate_versions(
            tokio_versions(),
            &current_version(),
            far_future_cutoff(),
            &requirements,
        );

        assert!(
            candidates.is_empty(),
            "expected no candidates, got {candidates:?}"
        );
    }
}
