# Documentation Structure

This document describes the organization and conventions for Sigilforge documentation.

## Directory Layout

```
docs/
├── STRUCTURE.md           # This file - documentation guidelines
├── ARCHITECTURE.md        # Current architecture documentation
├── INTERFACES.md          # Current API and trait definitions
├── ROADMAP.md             # Current development roadmap
├── NEXT_STEPS.md          # Current development tasks
├── RELEASE.md             # Release process documentation
└── versions/              # Versioned documentation snapshots
    └── v0.1.0/           # Documentation for v0.1.0 release
        ├── ARCHITECTURE.md
        ├── INTERFACES.md
        └── ROADMAP.md
```

## Documentation Categories

### Core Documentation (Root Level)

These files are always kept up-to-date with the current main branch:

- **STRUCTURE.md**: Documentation organization and conventions (this file)
- **ARCHITECTURE.md**: System architecture, component design, and technical decisions
- **INTERFACES.md**: Trait definitions, API contracts, and integration guides
- **ROADMAP.md**: Development phases and future plans
- **NEXT_STEPS.md**: Concrete next tasks for development
- **RELEASE.md**: Release process and versioning workflow

### Versioned Documentation (versions/)

Each release gets a snapshot of documentation in `docs/versions/vX.Y.Z/`:

- **Purpose**: Preserve documentation that matches released code
- **Contents**: Copy of ARCHITECTURE.md, INTERFACES.md, ROADMAP.md at release time
- **Naming**: Use semantic version format (v0.1.0, v1.0.0, etc.)

## Versioning Policy

### When to Create Version Snapshot

Create a new version directory when:
- Cutting a new release (major, minor, or patch)
- Significant API or architecture changes land in main
- Documentation diverges significantly from the latest release

### What to Include

Include in versioned docs:
- ARCHITECTURE.md (system design may evolve)
- INTERFACES.md (API contracts may change)
- ROADMAP.md (plans shift over time)

Do NOT version:
- STRUCTURE.md (meta-documentation about docs themselves)
- NEXT_STEPS.md (always reflects current development state)
- RELEASE.md (process documentation, not version-specific)

### vNEXT Convention

- Use `docs/versions/vNEXT/` for unreleased changes
- When cutting a release, rename vNEXT to the actual version number
- Immediately create a new vNEXT directory

## Content Guidelines

### Required Sections

All major documentation files should include:

1. **Title and Overview**: Brief description of the document's purpose
2. **Table of Contents**: For documents longer than 100 lines
3. **Examples**: Concrete usage examples where applicable
4. **Links**: Cross-references to related documentation

### Writing Style

- **Be concise**: Prefer short, clear sentences
- **Use code examples**: Show, don't just tell
- **Update examples**: Ensure code examples match current API
- **Link to source**: Reference actual source files when discussing implementation

### Maintenance

- **Review on PR**: Ensure docs are updated when code changes
- **Delete stale content**: Remove outdated examples and references
- **No orphaned files**: Every doc should be linked from README or another doc
- **No AI logs**: Remove raw conversation dumps; distill insights into guides

## CI Integration

Documentation checks run in CI to ensure:

1. **Structure validation**: Required files exist and are non-empty
2. **Link checking**: No broken internal links (future enhancement)
3. **Example validation**: Code examples compile (future enhancement)

## README Integration

The root README.md should:
- Link to core documentation files
- Point users to versioned docs for specific releases
- Include a quickstart guide
- Reference STRUCTURE.md for contributors

## Release Process Integration

When cutting a release:

1. Update version in Cargo.toml files
2. Copy current docs to `docs/versions/vX.Y.Z/`
3. Update CHANGELOG.md with release notes
4. Tag the release
5. Update README.md to reference the new version

See RELEASE.md for detailed release workflow.

## Archive Policy

We do NOT maintain archive directories:
- Old content is preserved in git history
- Versioned docs replace the need for archives
- AI conversation logs are deleted, not archived
- Legacy design docs are distilled into current documentation
