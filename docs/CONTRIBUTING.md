# Contributing to MemoBuild

We welcome contributions! Please review these guidelines before pushing PRs to the MemoBuild project.

## Development Workflow

1. Fork & branch your feature: `git checkout -b type/feature_name`.
2. Format the code: `cargo fmt`.
3. Check code styles: `cargo clippy --all-targets --all-features`.
4. Run testing frameworks: `cargo test --all-features`.
5. Pre-commit check out your security checks using our `scripts/security-audit.sh` file.

### Adding New Server Features
For API changes, bump the version documented in `docs/API_CHANGELOG.md` and test against earlier clients utilizing `tests/e2e_test.rs`. Do not push backward incompatible JSON schemas strictly immediately.

## Pull Requests

Submit pull requests directly attached to GitHub issues when appropriate. Wait for the `cargo check` and `.github` actions to pass before pinging maintainers internally.
