# Contributing to Basilica

Thanks for your interest in contributing to Basilica. We welcome contributions from the community.

The public `one-covenant/basilica` repository distributes the reviewed `public/`
tree from the private `one-covenant/basilica-backend` development monorepo.
Maintainers develop changes in the monorepo and export additive, signed public
commits. CLI/SDK release workflows, tags and package installation paths remain
in this public repository.

## Getting Started

Open an [issue in the public repository](https://github.com/one-covenant/basilica/issues)
with a clear reproduction, proposed change, or patch. Public PRs remain welcome
and accessible. Maintainers review and carry accepted changes into a private
monorepo PR with attribution, then regenerate this public tree. Existing PR
history remains available; do not merge independently authored changes directly
into the generated distribution.

Dependency version PRs for the public Rust workspace, Python SDK and validator
images are scheduled in the development monorepo and exported after review.
The public mirror does not schedule new version PRs; existing public PRs remain
available for maintainers to carry forward. Maintainers review the SHA pins in
the public release workflows manually in the monorepo before export, because
GitHub's Dependabot Actions scanner covers only repository-root workflows.

Maintainers working in the monorepo run the commands below from `public/`.
They also work from the repository root when validating a coordinated backend
and public change. In the standalone public repository, run them from its root.

## Development Setup

### Prerequisites

- Rust toolchain from `rust-toolchain.toml`
- Docker for container integration work
- CUDA toolkit for GPU-specific work

### Building

```bash
# Build all crates
cargo build

# Build specific crate
cargo build -p basilica-validator
cargo build -p basilica-miner

# Build with release optimizations
cargo build --release
```

### Testing

```bash
# Run all tests
cargo test

# Run tests for specific crate
cargo test -p basilica-common

# Run tests with logging
RUST_LOG=debug cargo test
```

## Code Standards

### Style Guidelines

- Follow Rust standard style guidelines
- Run `cargo fmt` before committing
- Run `cargo clippy` and fix all warnings
- Keep functions under 50 lines when possible
- Write self-documenting code with clear variable names

### Code Quality

```bash
# Format code
cargo fmt

# Check for linting issues
cargo clippy

# Fix linting issues and format
just fix
```

### Commit Messages

- Use clear, descriptive commit messages
- Start with a verb: "Add", "Fix", "Update", "Remove"
- Keep the first line under 50 characters
- Add detailed description if needed after a blank line

Example:
```
Fix weight calculation in validator scoring

- Correct GPU performance normalization
- Add bounds checking for weight values
- Update tests to cover edge cases
```

## Change Review

For a code change, run the relevant locked Cargo checks and include their
results in the issue or maintainer PR:

```bash
cargo test --locked
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
```

Link the public issue and explain any API or package compatibility effects.
Maintainers verify the changed monorepo component and its exported public tree
before updating this distribution or publishing a release.

## Testing Requirements

- All new features must include tests
- Bug fixes should include regression tests
- Maintain or improve code coverage
- Test edge cases and error conditions

## Documentation

- Update documentation for any API changes
- Add inline documentation for complex logic
- Update README if adding new features
- Include examples where helpful

## Security

- Never commit secrets or private keys
- Use environment variables for sensitive data
- Follow security best practices
- Report vulnerabilities via Discord

## Community

- Join our [Discord](https://discord.gg/Cy7c9vPsNK) for discussions
- Be respectful and constructive
- Help others when you can
- Share your ideas and feedback

## License

By contributing, you agree that your contributions will be licensed under the same license as the project.
