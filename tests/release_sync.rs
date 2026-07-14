//! The release checklist (CONTRIBUTING.md) encoded as tests: every file
//! that must carry the current version does, so a version-bump PR fails
//! until they are updated together. Cargo.lock is covered by `--locked`
//! in CI.

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn hermit_manifest_lists_current_version() {
    let manifest = include_str!("../cargo-cooldown-check.hcl");
    let version_block = manifest
        .lines()
        .find(|line| line.starts_with("version "))
        .expect("no version block in cargo-cooldown-check.hcl");
    assert!(
        version_block.contains(&format!("\"{VERSION}\"")),
        "cargo-cooldown-check.hcl version block is missing \"{VERSION}\" — hermit \
         consumers resolve versions from this manifest, not from the release list"
    );
}

#[test]
fn readme_action_example_pins_current_version() {
    let readme = include_str!("../README.md");
    assert!(
        readme.contains(&format!("cargo-cooldown-check@v{VERSION}")),
        "README's GitHub Actions usage example does not pin v{VERSION}"
    );
}
