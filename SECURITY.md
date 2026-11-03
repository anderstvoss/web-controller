# Security Policy

## Supported Versions

Security updates are provided for the latest version on the `main` branch unless otherwise stated.

## Reporting a Vulnerability

Please do not report security vulnerabilities through public GitHub issues.

**Preferred:** open a private report at
<https://github.com/anderstvoss/web-controller/security/advisories/new>
(GitHub Private Vulnerability Reporting).

**Alternative:** email the maintainer at
<anderstvoss@gmail.com>.

Include:

- Affected version or commit
- Reproduction steps
- Impact
- Suggested fix, if known

Initial response: within 7 days. Coordinated disclosure preferred; we
will work with you on a timeline before any public advisory.

## Defensive Posture

This repo enforces:

- Pre-commit secret scanning (gitleaks) plus a suite of custom blockers for
  env files, private keys, credentials, local paths, private IPs, cloud
  storage URIs, and binary artifacts.
- Pre-push: gitleaks full-tree scan + tracked-file blocker + local-paths
  guard + `cargo deny check` + `cargo audit`.
- `unsafe` is forbidden; if a crate ever needs FFI, every `unsafe` block
  must carry a `// SAFETY:` justification (`clippy::undocumented_unsafe_blocks`).
- Python helpers, if any, are linted for defects and security footguns
  (ruff pyflakes + bandit rules) and their unit tests run in CI.
- CodeQL (`security-extended`) for Rust and for the workflow files.
- CI on every PR and push to `main`: `cargo fmt`, `clippy -D warnings`,
  `cargo check`, `cargo test` on Ubuntu (Linux-only service); the full
  pre-commit + pre-push policy replay on Ubuntu + macOS + Windows; `cargo-deny`
  + `cargo-audit`; `actions/dependency-review-action` on PRs.
- All third-party actions SHA-pinned; `step-security/harden-runner`
  with egress-policy `block` and an explicit allowlist on every Linux
  job (multi-OS jobs use `audit` on macOS/Windows, where block mode
  breaks certificate revocation lookups).
- A disclosure job scans every file version and commit email reachable
  from any branch, tag or pull-request head; personal commit emails fail
  it, so commits use a GitHub noreply identity.
- Windows crate downloads retry a bounded number of times; certificate
  revocation checking is never disabled.
- Weekly full-history gitleaks scan over every ref (branches and
  tags), plus `scripts/deep-scan.sh` for ad-hoc operator runs.
- OpenSSF Scorecard on push to `main`, weekly cron, and
  branch-protection-rule events; SARIF published to the Security tab.
- Dependabot alerts + automated security updates.
- Codeowner review required on `.github/`, hooks, security docs, and
  dependency manifests.
- A full-history secret scan is a precondition for changing repository
  visibility.

## Threat surface of the service (planned)

The service lets LAN clients create and drive local input devices, so an
unauthorized client could inject input into the host. The design treats
every browser as untrusted: pairing must authorize device creation,
WebSocket messages are validated and rate- and size-limited, the origin is
checked, device counts are capped, and the service runs unprivileged with
narrowly scoped access to `/dev/uinput` and `/dev/uhid`. The detailed
threat model will live in `docs/SECURITY.md` once written. Until a release
says otherwise, run it only on a trusted network.

For the end-to-end setup procedure (reusable across projects), see
[`docs/REPO-SETUP.md`](docs/REPO-SETUP.md).
