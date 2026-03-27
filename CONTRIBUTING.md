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
3. Open a PR with the version bump.
4. Merge the PR to `main`.

The workflow automatically creates a git tag and GitHub release with platform tarballs. If the version in `Cargo.toml` matches an existing tag, the publish job is skipped.

### Dry-run a release

Add the `ci:release` label to a PR. This triggers the full build matrix and artifact review without publishing.
