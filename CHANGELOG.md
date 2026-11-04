# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0][keep-a-changelog], and
this project adheres to [Semantic Versioning 2.0.0][semver].

## [Unreleased]

### Added

- M2 isolated controller sessions, terminal QR/PIN invites, reusable LAN test
  invites, host-approved joins, reconnect grace, expiration and session diagnostics.
- Automated HTTP/WebSocket lifecycle tests using local clients and fake device
  owners, plus a separate two-phone input acceptance procedure.

- Initial M1 browser-to-Xbox 360-style uinput implementation, versioned full-state input
  contract, pairing/session checks, input watchdog, and explicit virtualgamepad owner.
- Static no-build mobile controller layout and Node-native frontend state tests.
- Repository baseline from `baseline-hardened-repo`: local hooks,
  visibility-gated CI, CodeQL, Scorecard, gitleaks history scan, SBOM and
  disclosure checks.
- Planning handoff (`docs/planning/repository-handoff.md`) and agent rules
  (`AGENTS.md`).

### Changed

- Build the frontend from strict TypeScript with locked npm dependencies,
  formatting, lint, property tests, audit, license, SBOM and CodeQL gates.

### Deprecated

### Removed

### Fixed

### Security

[keep-a-changelog]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html
[Unreleased]: https://github.com/anderstvoss/web-controller/commits/main
