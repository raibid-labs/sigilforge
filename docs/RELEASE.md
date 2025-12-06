# Release Process

This document describes how to create a new release of Sigilforge.

## Prerequisites

Before cutting a release, ensure:

1. All planned features/fixes are merged to `main`
2. CI is passing on `main`
3. Documentation is up-to-date
4. CHANGELOG.md is updated with release notes
5. Version numbers are bumped in Cargo.toml files

## Release Types

Sigilforge follows [Semantic Versioning](https://semver.org/):

- **Major (x.0.0)**: Breaking API changes
- **Minor (0.x.0)**: New features, backward compatible
- **Patch (0.0.x)**: Bug fixes, backward compatible

## Release Workflow

### 1. Prepare the Release

1. **Update version numbers** in all Cargo.toml files:
   ```bash
   # Update workspace version in root Cargo.toml
   # Update individual crate versions if needed
   ```

2. **Update CHANGELOG.md**:
   ```markdown
   ## [0.2.0] - 2024-01-15

   ### Added
   - New OAuth provider support for GitHub
   - Device code flow implementation

   ### Changed
   - Improved token refresh error handling

   ### Fixed
   - Race condition in daemon startup
   ```

3. **Create versioned documentation snapshot**:
   ```bash
   mkdir -p docs/versions/v0.2.0
   cp docs/ARCHITECTURE.md docs/versions/v0.2.0/
   cp docs/INTERFACES.md docs/versions/v0.2.0/
   cp docs/ROADMAP.md docs/versions/v0.2.0/
   ```

4. **Commit changes**:
   ```bash
   git add -A
   git commit -m "Prepare release v0.2.0"
   git push origin main
   ```

### 2. Create and Push Tag

Create an annotated tag for the release:

```bash
# Format: vMAJOR.MINOR.PATCH
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

### 3. Automated Release Process

When the tag is pushed, the release workflow automatically:

1. **Creates GitHub Release**:
   - Extracts changelog from CHANGELOG.md
   - Creates release notes
   - Marks as prerelease if version contains `-alpha`, `-beta`, etc.

2. **Builds Binaries** for multiple platforms:
   - Linux x86_64 (glibc)
   - Linux x86_64 (musl)
   - macOS x86_64
   - macOS ARM64 (Apple Silicon)
   - Windows x86_64

3. **Generates Checksums** (SHA256) for all artifacts

4. **Publishes to crates.io**:
   - `sigilforge-core`
   - `sigilforge-daemon`
   - `sigilforge-cli`

### 4. Verify Release

After the workflow completes:

1. Check GitHub Releases page for the new release
2. Verify all binary artifacts are attached
3. Verify checksums are present
4. Test installation from source:
   ```bash
   cargo install --git https://github.com/raibid-labs/sigilforge --tag v0.2.0
   ```
5. Verify crates.io publication:
   ```bash
   cargo search sigilforge
   ```

### 5. Post-Release

1. **Announce the release**:
   - Update ecosystem projects (Scryforge, Phage, etc.)
   - Post in relevant channels

2. **Monitor for issues**:
   - Watch for bug reports
   - Prepare patch release if critical issues found

## Manual Release (Emergency)

If automated release fails, you can manually trigger it:

1. Go to Actions tab in GitHub
2. Select "Release" workflow
3. Click "Run workflow"
4. Enter the version tag (e.g., `v0.2.0`)

## Publishing to crates.io

Publishing requires a `CARGO_TOKEN` secret in GitHub:

1. Generate token at https://crates.io/me
2. Add as repository secret: Settings > Secrets > Actions > New repository secret
3. Name: `CARGO_TOKEN`

## Branch Protection

The `main` branch has protection rules:

- **Require pull request reviews**: At least 1 approval
- **Require status checks**: CI must pass
- **Require branches to be up to date**: Prevent stale merges
- **Require signed commits**: (Optional) Enhanced security

To modify these settings:
1. Go to Settings > Branches
2. Edit branch protection rule for `main`

## CODEOWNERS

Release-related files require review from maintainers:

- `.github/workflows/release.yml`: Release workflow
- `docs/RELEASE.md`: This file
- `CHANGELOG.md`: Release notes
- `Cargo.toml`: Version changes

See `.github/CODEOWNERS` for the full list.

## Versioning Guidelines

### When to Bump Major Version

- Breaking changes to public API
- Removal of deprecated features
- Major architecture changes

### When to Bump Minor Version

- New features added
- New OAuth providers
- New CLI commands
- Deprecations (with backward compatibility)

### When to Bump Patch Version

- Bug fixes
- Documentation improvements
- Performance improvements (no API changes)
- Security fixes

## Hotfix Process

For critical bugs in production:

1. Create hotfix branch from release tag:
   ```bash
   git checkout -b hotfix/v0.2.1 v0.2.0
   ```

2. Fix the bug and commit

3. Update version to v0.2.1

4. Merge to main:
   ```bash
   git checkout main
   git merge hotfix/v0.2.1
   ```

5. Tag and push:
   ```bash
   git tag -a v0.2.1 -m "Hotfix: Fix critical bug"
   git push origin v0.2.1
   git push origin main
   ```

## Rollback

If a release has critical issues:

1. **Mark release as draft** on GitHub (doesn't delete artifacts)
2. **Yank from crates.io** if necessary:
   ```bash
   cargo yank --vers 0.2.0 sigilforge-core
   cargo yank --vers 0.2.0 sigilforge-daemon
   cargo yank --vers 0.2.0 sigilforge-cli
   ```
3. **Prepare hotfix release** following hotfix process

Note: Yanking prevents new projects from using the version but doesn't break existing projects.

## Troubleshooting

### Release workflow fails

1. Check workflow logs in Actions tab
2. Common issues:
   - Missing secrets (CARGO_TOKEN)
   - Compilation failures on specific platforms
   - Network issues with crates.io

### crates.io publish fails

- **Version already exists**: Cannot republish same version
- **Dependency resolution**: Ensure workspace dependencies are published in order
- **Token expired**: Regenerate CARGO_TOKEN

### Binary builds fail

- Check target platform specific dependencies
- Verify cross-compilation setup
- Test locally with:
  ```bash
  cargo build --release --target <target-triple>
  ```

## Checklist

Before releasing:

- [ ] All tests pass locally and in CI
- [ ] Documentation updated
- [ ] CHANGELOG.md updated with all changes
- [ ] Version bumped in Cargo.toml files
- [ ] Versioned docs created
- [ ] No uncommitted changes
- [ ] `main` branch is up-to-date

During release:

- [ ] Tag created and pushed
- [ ] Release workflow completed successfully
- [ ] GitHub release created with correct notes
- [ ] All binary artifacts attached
- [ ] Checksums generated
- [ ] crates.io publication succeeded

After release:

- [ ] Installation from tag tested
- [ ] crates.io listing verified
- [ ] Release announced
- [ ] Ecosystem projects notified
