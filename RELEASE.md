# Release Guide

This repository is public. Assume everything in git history, issues, and PRs is visible.

## Public Repo Safety

- Never commit API keys, tokens, credentials, or private URLs.
- Never put secrets in workflow YAML, scripts, or release notes.
- Use GitHub Actions encrypted secrets for CI/CD tokens.
- Rotate a secret immediately if it was ever exposed in a commit.

## Release Checklist

1. Ensure `main` is clean and up to date.
2. Run quality gates:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   ```
3. Bump the crate version in `Cargo.toml`:
   ```bash
   # edit Cargo.toml version = "X.Y.Z"
   cargo check
   ```
4. Commit and tag:
   ```bash
   git add Cargo.toml Cargo.lock
   git commit -m "release: vX.Y.Z"
   git tag vX.Y.Z
   git push origin main
   git push origin vX.Y.Z
   ```
5. Create a GitHub release:
   ```bash
   gh release create vX.Y.Z --generate-notes
   ```

## Homebrew Tap Release

Use a personal tap for distribution.

1. Create `schneidermayer/homebrew-tap` on GitHub.
2. Clone tap locally:
   ```bash
   git clone git@github.com:schneidermayer/homebrew-tap.git
   cd homebrew-tap
   mkdir -p Formula
   ```
3. Compute checksum for the release tarball:
   ```bash
   VERSION=X.Y.Z
   curl -L -o /tmp/catmd.tar.gz \
     "https://github.com/schneidermayer/catmd/archive/refs/tags/v${VERSION}.tar.gz"
   shasum -a 256 /tmp/catmd.tar.gz
   ```
4. Create `Formula/catmd.rb`:
   ```ruby
   class Catmd < Formula
     desc "cat-like CLI that renders Markdown with ANSI styling"
     homepage "https://github.com/schneidermayer/catmd"
     url "https://github.com/schneidermayer/catmd/archive/refs/tags/vX.Y.Z.tar.gz"
     sha256 "REPLACE_WITH_SHA256"
     license "MIT"

     depends_on "rust" => :build

     def install
       system "cargo", "install", *std_cargo_args(path: ".")
     end

     test do
       assert_match version.to_s, shell_output("#{bin}/catmd --version")
     end
   end
   ```
5. Validate formula locally:
   ```bash
   brew audit --strict Formula/catmd.rb
   brew install --build-from-source Formula/catmd.rb
   brew test catmd
   ```
6. Commit and publish tap:
   ```bash
   git add Formula/catmd.rb
   git commit -m "catmd vX.Y.Z"
   git push origin main
   ```
7. End-user install:
   ```bash
   brew tap schneidermayer/tap
   brew install schneidermayer/tap/catmd
   ```

## Notes

- Keep release tags immutable once published.
- Prefer `--locked` builds if dependency reproducibility becomes a requirement in your tap.
- If you automate release publishing, use least-privileged tokens only.
