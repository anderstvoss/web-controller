# M1: single-phone Xbox 360-style controller (implementation record)

Status: implemented and host-enumerated; SDL joystick input verified with a synthetic client. Mobile browser acceptance is pending. This file is subordinate to [the repository handoff](repository-handoff.md) and `AGENTS.md`.

## Minimum vertical slice

Phone touchscreen → static local webpage → authenticated same-origin WebSocket → full-state input snapshot → a single application-owned `virtualgamepad::Xbox360Controller` on explicit `linux.uinput` → Linux evdev.

M1 is **single-client** and only provides the Xbox 360 *style* Linux evdev controller. It does not implement XInput, host Bluetooth, Dolphin APIs, emulated motion, or the Steam Controller family. M2 adds QR onboarding, multi-session and graceful reconnect.

## Trust and lifecycle

- The operator provides a random eight-digit pairing code displayed as four digits, a hyphen, and four digits via runtime environment, never in repository files. The service requires an explicitly configured bind address and expected HTTPS origin; terminate TLS with a trusted, independently managed reverse proxy.
- A same-origin POST exchanges the pairing code for an HttpOnly, Secure, SameSite=Strict session cookie. Reject missing/foreign origins and enforce a single live socket.
- The only accepted WebSocket data is a versioned, bounded, validated full-state snapshot with a monotonically increasing sequence number; unknown fields and out-of-range floats are rejected. There is no client-controlled HID descriptor or realization selection.
- WebSocket network handling never owns the controller. A dedicated worker owns `virtualgamepad`, updates its native controls, commits changed snapshots, and services the controller independently, including at idle.
- The watchdog neutralizes a stale input state; disconnect neutralizes and closes the owned device. No automatic fallback or device persistence in M1.

## State protocol

JSON frames contain `v: 1`, `seq` (increasing unsigned integer), `client_ms` (browser monotonic milliseconds, never trusted as a host clock), `buttons` (15 booleans, enumerated below), `sticks` (four finite numbers in [-1,1]), and `triggers` (two finite numbers in [0,1]). Browser sends current full state at a regular rate and on meaningful control transitions.

`buttons`: A, B, X, Y, LB, RB, Back, Start, Guide, L3, R3, D-up, D-down, D-left, D-right.

`sticks`: left-X, left-Y, right-X, right-Y. Vertical axis is positive up. The frontend converts screen-down coordinate into negative Y. Backend clamps after validating. The uinput mapper converts vertical axes to Linux positive-down coordinates and scales both axes to signed 16-bit values; triggers scale to 0..255.

The protocol is intentionally not a cross-controller or IUCM schema. Future controller families will have separately versioned mappings without altering `virtualgamepad`.

## Verification gates

| Gate | Expected result | Evidence status |
|---|---|---|
| Parser / ranges / unknown fields / malformed shapes | Unit + property tests with no device | Passed locally |
| Frontend multi-touch, release on pointercancel/visibilitychange, analog clamping | Automated browser tests or mobile inspection | Pending |
| HTTPS reverse-proxy and session cookie | Only trusted origin can authenticate | See host evidence below |
| Virtual controller creation/cleanup | uinput device disappears on disconnect | Passed in local host probe |
| Native mapping | Correct evdev keys, d-pad, both sticks/triggers | Passed in SDL joystick probe |
| Real application | SDL/game sees inputs and releases | See host evidence below |

Do not mark M1 consumer-validated until the last two gates have recorded host evidence. Do not commit local TLS material, credentials, host addresses, or raw traffic.

## Dependency rationale

`virtualgamepad` is an exact pinned upstream Git dependency and the sole device backend. `axum` and `tokio` provide maintained HTTPS-reverse-proxy-compatible HTTP/WebSocket request handling and asynchronous connection isolation; `serde` and `serde_json` enforce the input contract; `rand` and `subtle` provide cryptographically random sessions and constant-time comparison. These are preferred over implementing WebSocket framing, HTTP parsing, or session cryptography manually.

## Deferred

M2: QR onboarding, multi-session assignment, reconnect grace, and packaged TLS onboarding. The end target is pairing without manual code entry. M3: IMU and DualSense. M4/M5: Wii MotionPlus and Ibex independent consumer gates.

## Host acceptance evidence, 2026-11-03

A local synthetic client tested the M1 daemon through a TLS reverse proxy on
Debian 13 arm64, Linux 6.12.113, and SDL 2.32.4 (Debian package
`2.32.4+dfsg-1`). The daemon ran as an unprivileged user with existing
uinput access. The client verified the temporary test certificate and used
masked WebSocket frames with generated input snapshots.

- HTTPS pairing rejected a foreign Origin and accepted the configured Origin.
  The session cookie carried Secure, HttpOnly, and SameSite=Strict.
  WSS rejected an invalid cookie and a foreign Origin.
- **Host-enumerated** via uinput. Each of two authenticated connections
  added one evdev node, and closing each connection removed that node.
- **Consumer-validated in SDL's joystick API with synthetic input.** SDL
  observed all 11 buttons and all four D-pad directions individually, plus
  simultaneous face buttons, both stick pairs, and both triggers.
  After 360 ms without frames, SDL read the initial neutral state.
- A follow-up directional probe found both vertical axes reversed in SDL.
  After converting protocol Y at the uinput boundary, SDL reported up/left
  as negative and down/right as positive for both sticks across two connections.
- Mobile browser certificate trust, cookie handling, multi-touch,
  pointer cancellation, visibility changes, and browser input rate and jitter
  still need a phone run. This probe sampled SDL state after each input and
  did not measure latency.
