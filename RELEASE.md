# Release Guide

This repository is public. Assume everything in git history, issues, and PRs is visible.

## Public Repo Safety

- Never commit API keys, tokens, credentials, or private URLs.
- Never put secrets in workflow YAML, scripts, or release notes.
- Use GitHub Actions encrypted secrets for CI/CD tokens.
- Rotate a secret immediately if it was ever exposed in a commit.

## Automation Setup (One-Time)

1. Ensure tap repository exists: `schneidermayer/homebrew-tap`.
2. Create a GitHub token with write access to `schneidermayer/homebrew-tap`.
3. Add token as secret `HOMEBREW_TAP_TOKEN` in `schneidermayer/catmd`.
4. Confirm workflow file exists: `.github/workflows/release.yml`.

## Automated Release Flow

1. Ensure `main` is clean and up to date.
2. Run quality gates:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   ```
3. Bump version in `Cargo.toml` and `Cargo.lock` to `X.Y.Z`.
4. Commit and push:
   ```bash
   git add Cargo.toml Cargo.lock
   git commit -m "release: bump version to vX.Y.Z"
   git push origin main
   ```
5. Tag and push:
   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

Pushing tag `vX.Y.Z` triggers the `Release` workflow, which:

- computes release tarball SHA256,
- creates/updates the GitHub release notes,
- updates `Formula/catmd.rb` in `schneidermayer/homebrew-tap`,
- commits and pushes tap changes.

## Verification

```bash
gh run list --workflow Release --limit 1
brew tap schneidermayer/tap
brew update
brew audit --strict --new --formula schneidermayer/tap/catmd
brew install schneidermayer/tap/catmd
catmd --version
```

## Manual Recovery (If Workflow Fails)

1. Re-run failed job from GitHub Actions.
2. If needed, update `Formula/catmd.rb` in the tap repo manually using the release tarball URL and SHA256.
3. Push tap fix and re-run `brew audit --strict --new --formula schneidermayer/tap/catmd`.

## Notes

- Keep release tags immutable once published.
- Prefer least-privileged tokens for automation.
