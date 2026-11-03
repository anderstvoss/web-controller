# Agent instructions

Rules for AI coding agents (and humans) working in this repository. Read
this before changing anything. The plan of record is
[docs/planning/repository-handoff.md](docs/planning/repository-handoff.md).

## Repository model

Single public repository. `main` is protected: every change goes through
a pull request with green required checks and linear history. There is no
mirror or sync step.

## Hard rules

- Never commit secrets, tokens, `.env` files, private keys, certificates,
  logs or device dumps. Local TLS material for testing stays untracked.
- Never commit local machine paths, usernames, private IPs, loopback
  endpoints, MAC or Bluetooth addresses, or device serials. Tests that
  must bind a loopback address narrow the `block-local-network-targets`
  hook with a per-path `exclude` limited to test directories and say why
  in the PR. Never delete a blocker hook.
- Do not edit `.github/`, `.githooks/`, `.pre-commit-config.yaml`,
  `SECURITY.md`, `LICENSE` or dependency policy files (`deny.toml`, future
  `web/.npmrc`) unless the task asks for it.
- Never rewrite published history or force-push.
- Commit with a GitHub noreply email; the Disclosure checks job fails on
  personal emails anywhere in reachable history.
- Justify every new dependency in the PR description.
- Fake or sanitised fixtures only. No real user data or captures.
- Add a regression test for every fixed bug at the lowest practical seam.
- Finish each task with a summary: what changed, which gates ran and
  their results, and what is left.

## Component rules (from the handoff)

- Do not change `virtualgamepad` from this repo. Demonstrate an upstream
  defect, then track it as a separate upstream issue or PR with tests.
- Pin `virtualgamepad` to an exact Git revision with `Cargo.lock`
  committed. Upgrades are a focused PR with a compatibility replay and a
  new evidence record.
- Do not reimplement controller HID protocols, report formats or USB or
  Bluetooth emulation in this service.
- Never silently substitute a different controller personality,
  realization or output mechanism (for example DSU) when a gate fails.
  A failed gate gets a recorded architecture decision.
- Status language is exact: **implemented**, **host-enumerated** or
  **consumer-validated**. Say "Xbox 360-style", never XInput emulation.
  Do not claim physical Bluetooth pairing, Puck or receiver support, or
  hardware parity for Steam trackpads, pressure or haptics.
- The device servicing loop must never block on the network or the
  frontend. Controllers are serviced on readiness and on their advertised
  deadlines, including when idle.
- Disconnect neutralizes input; session end destroys the owned device.
- Wii (M4-W/M5-W) and Steam Ibex (M4-S/M5-S) are independent lanes. A
  blocker in one never holds the other.

## Tests

- Unit and contract tests run in standard CI with no kernel devices.
- Tests that need `/dev/uinput` or `/dev/uhid` are separate, opt-in
  targets and are never required for public CI to pass.
- Any parser that crosses a trust boundary (WebSocket messages, pairing
  requests, config) gets property tests (`proptest`) and should get a
  fuzz target.

## Gates to run before every PR

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo deny check
cargo audit
pre-commit run --all-files
pre-commit run --all-files --hook-stage pre-push
```

Once `web/` exists, also run the frontend gates listed in
[docs/HARDENING-CHECKLIST.md](docs/HARDENING-CHECKLIST.md#frontend-web-onboarding).
