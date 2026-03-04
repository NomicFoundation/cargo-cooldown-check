use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Context;
use cargo_metadata::{Metadata, Node, Package, PackageId, camino::Utf8PathBuf};
use clap_cargo::{Features, Manifest};

use crate::{allowlist::Allowlist, config::Config};

pub const COOLDOWN_FILE_CONFIG: &str = "cooldown.toml";
pub const ALLOWLIST_FILE_CONFIG: &str = "cooldown-allowlist.toml";

#[derive(Debug)]
pub struct Workspace {
    pub packages: HashMap<PackageId, Package>,
    pub root_path: PathBuf,
    pub config: Config,
    pub allowlist: Allowlist,
    pub nodes: Vec<Node>,
}

impl Workspace {
    pub fn load(manifest_path: Option<&Path>) -> anyhow::Result<Self> {
        let features = {
            let mut features = Features::default();
            features.all_features = true;
            features
        };
        let mut manifest = Manifest::default();
        manifest.manifest_path = manifest_path.map(Path::to_path_buf);
        let metadata = read_metadata(&manifest, &features)?;
        let config_file_path =
            cargo_config_file_path(&metadata.workspace_root, COOLDOWN_FILE_CONFIG);
        let config = Config::load(&config_file_path)?;
        let allowlist_file_path =
            cargo_config_file_path(&metadata.workspace_root, ALLOWLIST_FILE_CONFIG);
        let allowlist = if allowlist_file_path.exists() {
            Allowlist::load(&allowlist_file_path)?
        } else {
            Allowlist::default()
        };

        let nodes = metadata
            .resolve
            .context("cargo metadata output did not include a resolved dependency graph")?
            .nodes;

        let packages = metadata
            .packages
            .into_iter()
            .map(|pkg| (pkg.id.clone(), pkg))
            .collect();

        let root_path = metadata.workspace_root.into();

        Ok(Self {
            packages,
            root_path,
            config,
            allowlist,
            nodes,
        })
    }
}

fn cargo_config_file_path(workspace_root_path: &Utf8PathBuf, filename: &str) -> PathBuf {
    let mut path = PathBuf::from(workspace_root_path);
    path.push(".cargo");
    path.push(filename);
    path
}

fn read_metadata(manifest: &Manifest, features: &Features) -> anyhow::Result<Metadata> {
    let mut command = manifest.metadata();
    features.forward_metadata(&mut command);
    let metadata = command.exec()?;
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// Creates a minimal Cargo project in a temp directory with no config files.
    fn minimal_cargo_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let cargo_dir = dir.path().join(".cargo");
        fs::create_dir_all(&cargo_dir).unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test-pkg"
version = "0.1.0"
edition = "2024"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        dir
    }

    fn write_cooldown_config(dir: &tempfile::TempDir) {
        fs::write(
            dir.path().join(".cargo").join(COOLDOWN_FILE_CONFIG),
            r#"
cooldown_minutes = 10080
"#,
        )
        .unwrap();
    }

    fn write_malformed_file(dir: &tempfile::TempDir, filename: &str) {
        fs::write(
            dir.path().join(".cargo").join(filename),
            r#"this is not valid toml {{{"#,
        )
        .unwrap();
    }

    #[test]
    fn load_fails_when_cooldown_config_does_not_exist() {
        let dir = minimal_cargo_project();
        let manifest_path = dir.path().join("Cargo.toml");

        let err = Workspace::load(Some(&manifest_path)).unwrap_err();
        assert!(err.to_string().contains("failed to read cooldown config"),);
    }

    #[test]
    fn load_fails_when_cooldown_config_is_malformed() {
        let dir = minimal_cargo_project();
        write_malformed_file(&dir, COOLDOWN_FILE_CONFIG);
        let manifest_path = dir.path().join("Cargo.toml");

        let err = Workspace::load(Some(&manifest_path)).unwrap_err();
        assert!(err.to_string().contains("TOML parse error"),);
    }

    #[test]
    fn load_defaults_allowlist_when_file_does_not_exist() {
        let dir = minimal_cargo_project();
        write_cooldown_config(&dir);
        let manifest_path = dir.path().join("Cargo.toml");

        let workspace = Workspace::load(Some(&manifest_path)).unwrap();

        assert!(workspace.allowlist.allow.exact.is_empty());
        assert!(workspace.allowlist.allow.package.is_empty());
    }

    #[test]
    fn load_fails_when_allowlist_is_malformed() {
        let dir = minimal_cargo_project();
        write_cooldown_config(&dir);
        write_malformed_file(&dir, ALLOWLIST_FILE_CONFIG);
        let manifest_path = dir.path().join("Cargo.toml");

        let err = Workspace::load(Some(&manifest_path)).unwrap_err();
        assert!(err.to_string().contains("failed to parse allowlist"),);
    }
}
