use std::{fs, path::Path};

use anyhow::Context;

const DEFAULT_REGISTRY_INDEX: &str = "registry+https://github.com/rust-lang/crates.io-index";
const DEFAULT_SPARSE_REGISTRY_INDEX: &str = "registry+sparse+https://index.crates.io/";

#[derive(Debug, Clone)]
pub struct Config {
    pub cooldown_minutes: u64,
    pub allowed_registries: Vec<String>,
}

impl Config {
    pub fn is_registry_allowed(&self, source: &str) -> bool {
        self.allowed_registries
            .iter()
            .any(|allowed| allowed == source)
    }

    pub fn load(file_path: &Path) -> anyhow::Result<Self> {
        let file_config = CooldownFileConfig::load(file_path)?;
        log::info!("Cooldown config: {file_config:?}");
        Ok(Config {
            cooldown_minutes: file_config.cooldown_minutes,
            ..Config::default()
        })
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct CooldownFileConfig {
    cooldown_minutes: u64,
}

impl CooldownFileConfig {
    fn load(file_path: &Path) -> anyhow::Result<Self> {
        let file_contents = fs::read_to_string(file_path).with_context(|| {
            format!(
                "failed to read cooldown config at {}\n\t\
                 hint: create a `cooldown.toml` in your `.cargo/` directory",
                file_path.display()
            )
        })?;
        let cooldown_config: CooldownFileConfig = toml::from_str(&file_contents)?;
        Ok(cooldown_config)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cooldown_minutes: 10080, // 7 days
            allowed_registries: default_allowed_registries(),
        }
    }
}

fn default_allowed_registries() -> Vec<String> {
    vec![
        DEFAULT_REGISTRY_INDEX.to_string(),
        DEFAULT_SPARSE_REGISTRY_INDEX.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_allowed_registries_include_sparse_and_git() {
        let config = Config::default();
        assert_eq!(config.allowed_registries, default_allowed_registries());
    }

    #[test]
    fn load_missing_file_returns_meaningful_error() {
        let path = Path::new("/nonexistent/.cargo/cooldown.toml");
        let err = Config::load(path).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("failed to read cooldown config at"),);
        assert!(msg.contains("hint: create a `cooldown.toml`"),);
    }

    mod cooldown_file_config {
        use std::io::Write;

        use tempfile::NamedTempFile;

        use super::*;

        #[test]
        fn load_reads_cooldown_and_ignores_retired_cache_keys() {
            let mut file = NamedTempFile::new().unwrap();
            writeln!(
                file,
                r#"
cooldown_minutes = 60
cache_dir = "/tmp/my-cache"
cache_ttl_seconds = 3600
                "#
            )
            .unwrap();

            let config = Config::load(file.path()).unwrap();
            assert_eq!(config.cooldown_minutes, 60);
        }
    }
}
