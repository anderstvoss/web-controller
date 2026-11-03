# web-controller

[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/anderstvoss/web-controller/badge)](https://scorecard.dev/viewer/?uri=github.com/anderstvoss/web-controller)

A Linux service that turns a smartphone into a local game controller with
no native app. The phone opens an HTTPS page served by the Linux host,
pairs to a session, and sends touchscreen and (where the browser allows)
motion-sensor input over an authenticated WebSocket. The service creates
an ordinary virtual controller through the
[`virtualgamepad`](https://github.com/anderstvoss/virtualgamepad) library,
so games, SDL applications and emulators see a normal local device.

> **Status:** M1 implementation candidate. The browser, input contract and
> Xbox 360-style uinput integration are under validation. Host enumeration,
> real game/SDL consumption, HTTPS deployment and required CI gates have not
> yet been demonstrated. See [CHANGELOG.md](CHANGELOG.md) for tracked changes.

## Scope

The service owns web input, pairing, sessions, mapping and device
lifecycle. `virtualgamepad` owns controller semantics, report encoding
and device realization; this repo does not reimplement controller HID
protocols. It runs and is tested on its own, independent of any launcher
or controller-management stack.

Planned targets, each gated on evidence from a real Linux consumer:

| Milestone | Target |
|---|---|
| M1 | Xbox 360-style controller via `linux.uinput` (touch only) |
| M2 | Multiple phones, QR pairing, reconnects, deployment |
| M3 | DualSense with phone motion via `linux.uhid.usb` |
| M4/M5 | Wii Remote + MotionPlus and Steam Controller 2026 (Ibex), as independent parallel lanes |

Status claims distinguish **implemented**, **host-enumerated** and
**consumer-validated**. Kernel enumeration alone is never reported as
working controller support.

The full plan, scope boundaries, risk register and open decisions are in
[docs/planning/repository-handoff.md](docs/planning/repository-handoff.md).
Contributors and coding agents should read [AGENTS.md](AGENTS.md) first.

## License

[AGPL-3.0-only](LICENSE), matching `virtualgamepad`, which this service
links.

## Setup

After cloning, run once:

```bash
cargo install cargo-deny cargo-audit
git config core.hooksPath .githooks
git config user.email '<id>+<login>@users.noreply.github.com'

# If you previously ran `pre-commit install` on this clone, remove the
# now-stale wrappers in .git/hooks/ so git only consults .githooks/:
rm -f .git/hooks/pre-commit .git/hooks/pre-push
```

`core.hooksPath` redirects git to the committed `.githooks/` directory.
The `pre-commit` wrapper delegates to the `pre-commit` Python package
(install via pipx or pip, see <https://pre-commit.com/#install>); the
`pre-push` wrapper runs gitleaks, the tracked-file blocker and the
local-paths scan, then hands off to pre-commit's pre-push hooks
(`cargo deny` + `cargo audit`).

## Development

```bash
cargo build
cargo test
```

Local gates (also run in CI on every PR):

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
gitleaks detect
ruff check .    # Python helpers under scripts/
```

The M1 browser interface currently uses no-build static ES modules in
`assets/`, with Node's built-in test runner; this is deliberately an interim
frontend. A future TypeScript `web/` migration must include the frontend
hardening profile before merging; see
[docs/HARDENING-CHECKLIST.md](docs/HARDENING-CHECKLIST.md#frontend-web-onboarding).

Before publishing or merging anything that matters, run a deep gitleaks
scan across all branches and tags:

```bash
scripts/deep-scan.sh
```

Record user-visible changes in [CHANGELOG.md](CHANGELOG.md) under
`Unreleased`.

## M1 browser-controller prototype

M1 accepts **one phone** at a time, maps full-state inputs through the pinned
`virtualgamepad` Git dependency, and creates a Linux Xbox 360-style uinput
controller. This is **not** Microsoft's XInput USB protocol. For the contract
and unverified acceptance gates, see
[docs/planning/m1-implementation.md](docs/planning/m1-implementation.md).

The Rust service intentionally does not terminate TLS. Configure a **trusted
HTTPS reverse proxy**, restrict the backend listener to the proxy, and set:

```sh
export WEB_CONTROLLER_BIND='<proxy-accessible-bind-address>:<port>'
export WEB_CONTROLLER_ORIGIN='https://<trusted-hostname>'
export WEB_CONTROLLER_PAIR_CODE="$(python3 -c 'import secrets; print(f"{secrets.randbelow(10000):04d}-{secrets.randbelow(10000):04d}")')"
# Display this code to the operator locally; never save it in Git.
printf '%s\n' "$WEB_CONTROLLER_PAIR_CODE"
cargo run
```

On a phone that can reach the trusted HTTPS origin, open the root page, enter
the eight-digit pairing code (with or without the hyphen), and press Connect. The page presents touchscreen buttons,
dual analog sticks, and triggers. The virtual controller is created on
connection; stale input is neutralized after a watchdog timeout, and the
controller is destroyed when the WebSocket closes.

Operational requirements: Linux `/dev/uinput` must be present and accessible
to the service account without running the HTTP service as root. Only expose
the HTTPS proxy to trusted clients; never forward the unencrypted backend to
the LAN. This prototype does not provision certificates, trust stores,
reverse-proxy settings, host ACLs or QR codes. No development credentials
are built into the frontend.

Frontend state tests require only Node.js:

```sh
node --test assets/input-model.test.mjs
node --check assets/app.mjs
```

The full Phase 1 acceptance gate requires an actual Linux host, evdev
inspection, and an SDL/game consumer test. Until those tests are recorded,
the implementation must not be described as consumer-validated.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Security

See [SECURITY.md](SECURITY.md). The hardening procedure is in
[docs/REPO-SETUP.md](docs/REPO-SETUP.md), with a tickable list in
[docs/HARDENING-CHECKLIST.md](docs/HARDENING-CHECKLIST.md).
