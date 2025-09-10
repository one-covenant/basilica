# Changelog

All notable changes to the basilica-cli package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0]

### Added
- Use registered callback ports for OAuth flow instead of dynamic port allocation
- Add coloring to clap help page with clap v3 styles for better readability
- Automatic authentication prompts when commands require auth (no manual login needed)
- GPU requirements-based selection - specify GPU needs and auto-select matching executors

### Changed
- Simplified token storage from keyring to file-based system
- Enhanced GPU executor display with grouped selection mode, compact view by default (use `--detailed` flag for full GPU names), and improved table formatting
- Unified GPU executor targeting - accept either executor UUID or GPU category (h100, h200, b200) as target parameter, removing separate --gpu-type option

### Fixed
- Consistent GPU count prefixes in all displays (e.g., "2x H100")
- Better expired/invalid token handling with clear user guidance

## [0.1.1] - Previous Release

Initial release
