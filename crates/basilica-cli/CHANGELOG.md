# Changelog

All notable changes to the basilica-cli package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1]

### Added
- New `tokens` command for API token management:
  - `tokens create` - Create a new API token with optional name and scopes
  - `tokens list` - List all API tokens
  - `tokens revoke` - Revoke the current API token
  - Basilica API now supports authentication via API tokens generated via this in addition to JWT

### Removed
- Deprecated `export-token` command (replaced by the new `tokens` subcommands)

## [0.3.0]

### Added
- Country-based filtering with `--country` flag for both `ls` and `up` commands (e.g., `--country US`)
- Hardware profile display with CPU model, cores, and RAM in executor details
- Enhanced rental list (`rentals` command) with CPU specs, RAM, and location information in detailed view
- Network speed information display for rentals and executors

### Changed
- GPU type filtering now uses direct parameter instead of `--gpu-type` flag (e.g., `basilica ls h100` instead of `basilica ls --gpu-type h100`)
- Simplified GPU display by removing memory information - now shows count and type only (e.g., "2x H100" instead of "2x H100 (80GB)")
- Executor list now groups by country with full country names instead of codes
- Improved rental list display with compact (default) and detailed (`--detailed`) views
- Location display now uses country names from basilica-api's country mapping for better readability

### Fixed
- Performance improvement: eliminated N+1 database queries when listing rentals

## [0.2.0]

### Added
- Use registered callback ports for OAuth flow instead of dynamic port allocation
- Add coloring to clap help page with clap v3 styles for better readability
- Automatic authentication prompts when commands require auth (no manual login needed)
- GPU requirements-based selection - specify GPU needs and auto-select matching executors
- `export-token` command for exporting authentication tokens in various formats (env, json, shell) for automation

### Changed
- Simplified token storage from keyring to file-based system
- Enhanced GPU executor display with grouped selection mode, compact view by default (use `--detailed` flag for full GPU names), and improved table formatting
- Unified GPU executor targeting - accept either executor UUID or GPU category (h100, h200, b200) as target parameter, removing separate --gpu-type option
- Migrated from basilica-api to basilica-sdk for all API interactions
- Refactored authentication to use oauth2 crate for improved token refresh mechanism
- Restructured authentication flow between SDK and CLI for better separation of concerns

### Fixed
- Consistent GPU count prefixes in all displays (e.g., "2x H100")
- Better expired/invalid token handling with clear user guidance
- Improved token refresh reliability with oauth2 crate integration

## [0.1.1] - Previous Release

Initial release
