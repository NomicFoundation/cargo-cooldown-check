# Contributing

## Release flow

The repository uses a GitHub Actions workflow (`.github/workflows/release.yml`) to build and publish releases. The workflow operates in three modes depending on the trigger:

| Trigger | What runs |
|---------|-----------|
| PR (default) | `ci.yml` only (check, test, cooldown-check) |
| PR + `ci:release` label | `ci.yml` + full 5-target cross-compile build + artifact review |
| Push to `main` | Full build + review + publish (if `Cargo.toml` version > latest tag) |

The build matrix covers five platforms: Linux x86/ARM, macOS x86/ARM, and Windows x86.

### Versioning

The binary and the GitHub Action share a single version in `Cargo.toml`. Bumping the binary version naturally picks up any action changes as well. However, if a change only touches the action (e.g. `action.yml`) without modifying the binary, you should still bump the version and cut a new release so consumers can pin to it.

### How to release

1. Bump the version in `Cargo.toml`.
2. Run `cargo check` so `Cargo.lock` picks up the new version.
3. Add the new version to the `version` block in `cargo-cooldown-check.hcl` — hermit resolves versions from the manifest on `main`, not from the release list, so consumers cannot pin the new version without this.
4. Update the pinned version in the README's GitHub Actions usage example.
5. Open a PR with the version bump.
6. Merge the PR to `main`.

Steps 2–4 are CI-enforced: `--locked` catches a stale `Cargo.lock`, and `tests/release_sync.rs` fails until the hermit manifest and README carry the new version.

The workflow automatically creates a git tag and GitHub release with platform tarballs. If the version in `Cargo.toml` matches an existing tag, the publish job is skipped.

Each publish also regenerates the `index` release (a hermit search index consumed by Renovate — see the Hermit / Renovate section in the README). Do not delete this release; it is reused across versions and updated in place.

After a release, downstream consumers pick up the new version via Renovate, or by hand: edr and solx pin the GitHub Action by commit SHA; slang pins the hermit package in its `bin/`.

### Dry-run a release

Add the `ci:release` label to a PR. This triggers the full build matrix and artifact review without publishing.
