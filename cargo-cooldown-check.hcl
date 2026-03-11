description = "Checks that all Cargo dependencies have been published for a minimum cooldown period"
binaries = ["cargo-cooldown-check"]
test = "cargo-cooldown-check --help"

version "0.1.2" {
  auto-version {
    github-release = "NomicFoundation/cargo-cooldown-check"
  }
}

platform "linux-amd64" {
  source = "https://github.com/NomicFoundation/cargo-cooldown-check/releases/download/v${version}/cargo-cooldown-check-v${version}-x86_64-unknown-linux-gnu.tar.gz"
}

platform "linux-arm64" {
  source = "https://github.com/NomicFoundation/cargo-cooldown-check/releases/download/v${version}/cargo-cooldown-check-v${version}-aarch64-unknown-linux-gnu.tar.gz"
}

platform "darwin-amd64" {
  source = "https://github.com/NomicFoundation/cargo-cooldown-check/releases/download/v${version}/cargo-cooldown-check-v${version}-x86_64-apple-darwin.tar.gz"
}

platform "darwin-arm64" {
  source = "https://github.com/NomicFoundation/cargo-cooldown-check/releases/download/v${version}/cargo-cooldown-check-v${version}-aarch64-apple-darwin.tar.gz"
}
