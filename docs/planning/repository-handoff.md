# Web Controller Service — Repository Planner Handoff

**Status:** Planning / repository preparation (revision 2: Steam Controller Ibex target added)
**Date:** 2026-11-03
**Provisional repository name:** `web-controller` (not approved as a final name)
**Intended audience:** Coordination planner responsible for repository initialization, state management, issue orchestration, and delivery tracking
**Implementation ownership:** Separate engineering conversation; **do not begin feature implementation as part of this handoff**

## 1. Mission and rationale

Create a standalone, Linux-hosted service that lets smartphones function as local game controllers **without installing a native phone app**. A phone opens an HTTPS webpage served by the Linux host, pairs to a session, and transmits touchscreen and (where supported) inertial-sensor inputs. The service uses the existing Rust [`virtualgamepad`](https://github.com/anderstvoss/virtualgamepad) library to instantiate a controller locally through Linux input/HID facilities.

Primary reasons for the project:

- **Low-friction access:** Any supported phone can join from a browser, ideally by scanning a pairing QR code. A controller can be supplied to guests without installing software or obtaining special mobile privileges.
- **Local compatibility:** Existing Linux games, Steam/SDL applications, and emulators should see a **locally created virtual device**, not need to understand the webpage or network protocol.
- **Progressive capabilities:** Start with conventional touch buttons/sticks; add phone gyroscope and accelerometer input; eventually target **both Wii Remote + MotionPlus and Steam Controller 2026 (Ibex)**, with phone-supplied motion and touch surfaces.
- **Clean separation:** The service owns web input, pairing, sessions, mapping, and device lifecycle. `virtualgamepad` owns controller-specific semantics, protocol/report encoding, and realization. Avoid reimplementing controller HID protocols.
- **Future flexibility:** Additional compiled controller families, touchscreen layouts, and input transports may be added without making this service depend on an overarching launcher architecture.

This service is **independent of IUCM, GamepadManager, the multi-instance launcher, Dolphin, and Steam**. It can be integrated with those systems later, but it must operate and be tested by itself.

### Intended user experience

1. Start the service on a Linux computer with appropriate device permissions.
2. Open a pairing URL or scan a QR code using a smartphone on the reachable LAN.
3. Authenticate/accept the controller session and select a supported controller presentation.
4. Use touchscreen controls and, for motion-capable presentations, grant browser motion permission.
5. Linux applications receive input from an ordinary virtual controller managed by `virtualgamepad`.
6. Disconnection neutralizes stale input; temporary reconnection preserves device identity when feasible; session termination destroys the owned virtual controller.

## 2. Scope boundaries

### In scope

- HTTPS static-web serving and an authenticated local WebSocket input channel.
- Mobile browser UI: touch buttons, D-pad, analog stick regions, triggers, controller layout selection, status, motion permission, and calibration/recenter controls as required by a target.
- Session and pairing management, authorization of virtual device creation, reconnect grace periods, rate/size limits, and session diagnostics.
- Versioned input messages with control states, sequence numbers, timestamps, explicit IMU units/frames, and capability detection.
- Bounded input ingestion, latest-state behavior for continuous controls, reliable release behavior for buttons, stale-input watchdogs, and safe neutralization.
- Explicit creation/servicing/teardown of `virtualgamepad` instances for approved controller family + realization combinations, including a future Steam Controller Ibex wired-style personality.
- Reproducible packaging, host preflight, local testing, and evidence-backed validation against actual Linux consumers.

### Explicit non-goals for the MVP

- IUCM integration, GamepadManager player slots, multi-instance-launcher management, or arbitrary composition of physical controllers.
- General controller-profile scripting, user-defined HID descriptors, automatic realization fallback, or adding new controller families inside this service.
- Bluetooth transport, Bluetooth PAN, Web Bluetooth, WebRTC, internet-hosted relay, NAT traversal, remote/cloud gaming, or creating a Wi-Fi access point.
- A native iOS/Android app, game discovery, Steam integration, or Dolphin-specific control APIs.
- Claims of actual Xbox XInput USB-device emulation, physical Bluetooth Wii Remote pairing, or guaranteed low-latency motion behavior.

Potential future enhancements must be proposed and independently scoped rather than built into the initial architecture by default.

## 3. Verified upstream integration facts and constraints

The upstream repository inspected during planning is [`anderstvoss/virtualgamepad`](https://github.com/anderstvoss/virtualgamepad), with documentation on its default `main` branch. **Re-read the source at the commit chosen for dependency pinning**; the library is evolving, and documentation may change.

- `virtualgamepad` is a Rust library with compiled, curated controller families. Its ordinary-root application API uses explicit creation targets and native control operations such as `create_xbox360`, `create_dualsense`, `CreationOptions::new(RealizationId::LINUX_UINPUT / LINUX_UHID_USB)`, `set_native`, `commit`, `service`, `neutralize`, and `close`.
- `linux.uinput` creates Linux evdev input devices through `/dev/uinput`.
- `linux.uhid.usb` presents a **local HID personality with USB bus metadata** through `/dev/uhid`; it does **not** create a literal physical USB device.
- The Xbox 360 presentation is **standard HID/evdev, not Microsoft's XInput/xpad USB protocol**. Use “Xbox 360-style” or “Xbox 360 family via uinput” in milestone claims.
- The DualSense implementation and its motion capabilities are work-in-progress and require explicit consumer acceptance; successful virtual creation is not proof of working application gyro input.
- A Wii Remote **supporting package** already exists with buttons, accelerometer, IR points, Nunchuk/Classic attachments, and MotionPlus inputs. Its `linux.uhid.bluetooth` is Bluetooth **bus metadata**, not a real BLE/Bluetooth connection; Dolphin/SDL consumer behavior and calibrated motion are not yet proven. The Wii package interface is outside the ordinary-root compatibility snapshot and must be pinned/tested deliberately.
- **Steam Controller 2026 (Ibex)** (`28de:1302`) is a distinct planned `virtualgamepad` family, **not a validated production realization**. The upstream wired-family plan targets `linux.uhid.usb` as the primary Steam/hid-steam/SDL-facing realization and `linux.uinput` as a reduced evdev alternative. An upstream November 2 host probe observed the UHID Ibex device bind to `hid-steam` and open in SDL with neutral input; it did **not** establish per-control input delivery, complete feature-report behavior, or Steam client listing. The limited stub did not list in Steam. The Puck (`28de:1304`), receiver (`1305`), actual Bluetooth, and wireless presentation are out of scope for this target.
- A live controller is a stateful protocol endpoint: it must be serviced on readiness **and** advertised monotonic deadlines, including during idle periods. The frontend or WebSocket handler must never block that servicing loop.
- The library does not own pairing, persistent app sessions, physical input routing, arbitrary player assignment, or filesystem persistence. These remain application responsibilities where needed.
- The upstream library is AGPL-3.0-only and currently Git-oriented (`publish = false` at the inspected state). Confirm distribution/licensing implications and pin the exact Git revision with lockfile. Avoid copying implementation source.

Canonical sources:

- [README](https://github.com/anderstvoss/virtualgamepad/blob/main/README.md)
- [Application API](https://github.com/anderstvoss/virtualgamepad/blob/main/docs/APPLICATION_API.md)
- [Core architecture](https://github.com/anderstvoss/virtualgamepad/blob/main/docs/CORE_ARCHITECTURE.md)
- [Controller support matrix](https://github.com/anderstvoss/virtualgamepad/blob/main/docs/CONTROLLER_SUPPORT.md)
- [Wii Remote supporting package](https://github.com/anderstvoss/virtualgamepad/blob/main/docs/WII_REMOTE.md)
- [Steam Controller Ibex wired-family plan](https://github.com/anderstvoss/virtualgamepad/blob/main/docs/plans/pre-controller-buildout/14-steam-controller-wired-package.md)
- [Steam UHID consumer probe](https://github.com/anderstvoss/virtualgamepad/blob/main/docs/plans/pre-controller-buildout/03-steam-uhid-consumer-probe.md)
- [Steam host probe results (2026-11-02)](https://github.com/anderstvoss/virtualgamepad/blob/main/docs/plans/pre-controller-buildout/HOST_SETUP_HANDOFF.md)
- [Deployment and validation](https://github.com/anderstvoss/virtualgamepad/blob/main/docs/DEPLOYMENT_AND_VALIDATION.md)

## 4. Proposed architecture

```text
Smartphone browser
  ├─ HTML/TypeScript UI (touchscreen layouts)
  ├─ DeviceMotion sensor interface, permission and calibration
  └─ HTTPS page + authenticated WebSocket
                          │
                          ▼
Standalone Rust service on Linux
  ├─ HTTP/static web server + pairing endpoint
  ├─ WebSocket transport and authenticated sessions
  ├─ Versioned input decoder + bounded latest-state store
  ├─ Controller-specific mapping adapters
  │    ├─ Xbox360-style touch → native controls
  │    ├─ DualSense touch + IMU → native controls
  │    ├─ Wii Remote touch + IMU → native controls (future)
  │    └─ Steam Ibex trackpad/touch + IMU → native controls (future)
  └─ Virtual controller owner / scheduling loop
       └─ virtualgamepad (pinned dependency)
            ├─ linux.uinput → evdev controller
            ├─ linux.uhid.usb → DualSense HID personality
            ├─ linux.uhid.bluetooth → Wii supporting path (unproven consumer acceptance)
            └─ linux.uhid.usb → Steam Ibex wired-style target (upstream WIP)
                          │
                          ▼
        Linux kernel drivers → apps/emulators
```

Prefer a single Rust service executable and one Rust crate initially, with modules defining the seams. The frontend can be a small TypeScript/Preact application built to static assets, served by the backend. Controller-family layouts should be independently selectable: the Steam Ibex layout will likely need **two touchpad regions, touch/click tracking, grip buttons, triggers, and IMU**. Do not presume that ordinary browser touchscreens offer the Steam Controller’s full pressure-sensor or haptic hardware fidelity. Suggested backend stack: Rust, Tokio, Axum, Serde, tracing. These are provisional implementation choices, not mandatory dependencies.

### Responsibilities and ownership

| Component | Owns | Must not own |
| --- | --- | --- |
| Browser frontend | UI, touch tracking, sensor access, local presentation | HID report formatting, Linux device creation |
| Network/session layer | HTTPS, auth, client IDs, sessions, reconnection, message validation | Direct mutable controller handles |
| Input/state layer | Canonical input snapshots, timestamps, validation, watchdog state | Universal physical-controller routing |
| Family mapper | Conversion to exact `virtualgamepad` native control types/units | Controller wire protocol or USB/Bluetooth emulation |
| Device owner | Per-instance lifetime, `commit`, `service`, deadlines, output observations, close | Network handshakes and browser rendering |
| `virtualgamepad` | Protocol-native behavior and exact Linux realization | Web UX, pairing, application session policy |

One authenticated phone session owns one logical virtual controller in the first version. The service is designed to *eventually* admit several concurrent sessions, each with isolated device ownership; no shared controller source composition is needed.

### Device lifecycle and concurrency

- A device-owning worker/task is the sole mutator of each controller handle. Network tasks push bounded commands or replace latest input state.
- Do not make `service()` depend on incoming input frames: schedule readiness/deadline processing independently.
- Maintain **both** a connection/session identity and a virtual-device identity. Temporary WebSocket reconnect should not necessarily destroy/recreate the device.
- On stale input or lost connection, neutralize pressed buttons and active axes promptly; do not leave a stuck throttle or button. Preserve a bounded reconnect grace period; close on session expiry or fatal library error.
- Prefer current, timestamped motion samples over replaying backlogs. Maintain deliberate handling of discrete press/release transitions and periodic full button-state snapshots so packet/connection loss cannot leave stuck controls.
- Do not assume virtualgamepad `commit()` means a downstream application consumed that state. Capture separately what is accepted, submitted, and consumer-validated.

## 5. Web protocol, security, and operational rules

### Initial wire contract

Maintain a versioned schema documenting: session ID/authorization (not a reusable secret in telemetry); protocol version; monotonically increasing sequence; client monotonic timestamp; full button bitset; normalized axes/triggers; optional touch contacts; optional IMU values; capabilities; and visibility/suspension signals.

Keep control names limited to what the frontend needs. Controller family mapping occurs server-side, not in the browser's network protocol. For Ibex, the contract must support **position, touch begin/move/end, multi-contact identity where available, touchpad-click intent, and pointer-pressure availability metadata**. True controller pressure and force-feedback/haptics are not guaranteed by a phone browser; label any simulation explicitly. Define axis orientations, zero points, clamping, dead zones, units, and behavior for missing sensors.

For browser `DeviceMotionEvent`, angular rates are specified in **degrees/second** and acceleration in **m/s²**. Any internal conversion (for example to rad/s) must be explicit, and device-body frame/orientation handling must be calibrated. Prefer `accelerationIncludingGravity` for gravity-sensitive Wii-style tilt where available; distinguish it from linear acceleration. Availability, event rate, and axis conventions vary across phone/browser implementations.

### Security posture

- Use HTTPS for sensor access; iOS requires a user-initiated motion permission flow. Provide a credible local certificate/trust strategy rather than rely on a self-signed warning bypass. A publicly trusted domain with local DNS resolution is one option.
- First release targets reachable **local Wi-Fi/LAN** only. No internet connection is needed to transmit controller packets once the secure page and trust path work.
- Pair with short-lived, single-use capability tokens or equivalent authenticated session establishment. Bind controller creation to authorized sessions, limit per-client/per-host device counts, and reject unrecognized WebSocket origins.
- Rate-limit and validate messages, enforce maximum sizes, neutralize on heartbeat timeout, and avoid arbitrary user-supplied HID reports or privileged host actions.
- Prefer running the service as an unprivileged user with narrowly scoped access to `/dev/uinput` and `/dev/uhid`, not as root. Ship a host-permission preflight and document installation steps; never silently mutate device ACLs on start.
- Avoid secrets, persistent session tokens, TLS private keys, personal sensor logs, or generated build output in Git. Describe retention/default-off telemetry in docs.

### Timing strategy

Start with WebSocket over LAN and a configurable motion/input send target around 60 Hz, while transmitting meaningful button changes immediately. Measure the browser's *actual* sensor event rate; do not promise 60/100 Hz delivery. Design the server around bounded work and fresh state, and instrument browser sampling, interarrival jitter, RTT, queuing, commit intervals, and application-visible events. Do not claim RTT is end-to-end motion-to-photon latency.

WebRTC, BLE, Wi-Fi AP provisioning, and native applications are optional later investigations, not MVP infrastructure.

## 6. Target roadmap and gated acceptance

### M0 — Repository and architecture baseline

Create/initialize repo after authorization; document scope, ADRs, build requirements, dependency pinning, API boundaries, security stance, and test plan. Add a minimal, reproducible Rust/TypeScript project scaffold and automated checks **only when implementation work is authorized**.

**Exit gate:** planner can show a coherent repository, an issue/milestone graph, the pinned upstream dependency decision, and an independently reviewed architecture baseline. No claim of controller functionality.

### M1 — First complete input path (Xbox 360-style via `linux.uinput`)

Touchscreen controls → authenticated WebSocket → validated state → native Xbox-family mapper → `virtualgamepad` uinput → Linux-visible evdev device. Support buttons, D-pad, shoulders, triggers, dual sticks, Start/Back, and stick clicks. Implement neutralization/cleanup from the start.

**Exit gate:** actual browser-to-Linux input demonstrated with evdev inspection and a real SDL/game consumer. Correct axes/buttons, disconnect cleanup, and no permanently stuck controls. Do **not** claim genuine XInput USB emulation.

### M2 — Multi-session robustness / deployment

Add secure pairing and QR workflow, reconnects, controller isolation, bounded queues, device limits, deterministic session expiration, diagnostics, reliable local host packaging, and testing with multiple phones.

**Exit gate:** multiple independent controllers can be created, controlled, disconnected, reconnected, and destroyed without cross-session input leakage or resource leaks.

### M3 — Motion-capable DualSense via `linux.uhid.usb`

Add client IMU capture, permissions, coordinate transforms, calibration/recenter operations, native DualSense IMU mapping, and optional touchpad representation. Exercise the upstream controller's required servicing cadence.

**Exit gate:** actual application demonstrates meaningful DualSense gyro/accelerometer behavior, with reported sampling rate/jitter and clear device/browser limitations. Kernel enumeration alone is insufficient.

### M4 — Parallel advanced-controller consumer feasibility gates: Wii and Steam Ibex

Run two **independent** investigations once the common session, controller-owner, and motion infrastructure exist. They share diagnostic infrastructure but **neither gate depends on the other succeeding**; both are scheduled at the same roadmap stage.

**M4-W — Wii Remote:** Before building a full Wii UI, create a controlled integration experiment using the upstream supporting Wii Remote implementation and chosen realization. Assess Linux binding, SDL HIDAPI/Dolphin recognition, MotionPlus/extension initialization, raw value mapping, and minimum required calibration resources.

**Wii exit gate:** either (a) reproducible consumer acceptance supports the planned path, or (b) documented incompatibility triggers a separate architecture decision. Do not silently swap to DSU or another output mechanism without re-scoping approval.

**M4-S — Steam Controller 2026 (Ibex):** Recheck the upstream Steam wired-family implementation and readiness at the exact pinned dependency SHA. Target `linux.uhid.usb` for the wired `28de:1302` personality; evaluate `linux.uinput` only as an explicitly reduced alternative. Verify `hid-steam` binding, SDL recognition, feature/report handshake, individual button/trackpad/IMU delivery, and Steam-client detection separately. **The existing neutral-only UHID/SDL probe clears a narrow feasibility question, not this acceptance gate.**

**Steam exit gate:** exact controls and motion are consumer-validated, with any unsupported pressure/haptics/Steam-client behavior recorded. If upstream Ibex isn't yet callable from the chosen pinned library, track the upstream dependency and hold this lane; do not implement Steam HID protocol inside the web service or silently substitute a different controller personality.

### M5 — Parallel web-controller buildout: Wii Remote + Steam Ibex

Build two independent family mappers and frontends **in parallel**, following their respective M4 gates. They may share input snapshot handling, coordinate transforms, network/session plumbing, and test infrastructure, but should maintain distinct controller-native semantics and UX.

**M5-W — Wii Remote + MotionPlus:** Implement portrait Wii Remote buttons, gravity/gyro mapping, recenter, optional Nunchuk layout, and IR pointing strategy. Treat absolute IR pointing as distinct from gyro angular velocity; a gyro-derived cursor drifts and is not equivalent to an IR camera with sensor bar.

**Wii exit gate:** Wii-specific game cases validate actual MotionPlus, shake/tilt, optional Nunchuk, and appropriately qualified pointing behavior. No physical Bluetooth pairing claim.

**M5-S — Steam Controller 2026 (Ibex):** Implement a landscape layout with dual trackpad touch and click, buttons, grips, triggers, optional analog stick, and phone IMU. Map browser touch contact position/gesture into native Ibex controls. Trackpad click can be a deliberate UI affordance rather than relying on touchscreen pressure; mark pressure/force-feedback/haptic parity unsupported until separately demonstrated. Retain independent decisions about device-host feedback and phone vibration (browser haptics may not exist).

**Steam exit gate:** verify left/right trackpad events, touch-release semantics, button/trigger/stick behavior, gyroscope/accelerometer behavior and device cleanup through actual `hid-steam`/SDL consumers. Run Steam-client acceptance as a separate, explicitly recorded test. Do not claim Puck, receiver, wireless/Bluetooth or full physical Steam Controller parity.

**Milestone scheduling rule:** Wii and Steam Ibex are *coequal* M4/M5 lanes, not a serial Wii-then-Steam sequence. Any Ibex upstream readiness blocker should not hold Wii, and any Wii/Dolphin blocker should not hold Ibex.

## 7. Repository initialization and management plan

**Do not assume the repo already exists or that its name, visibility, license, or organization has been approved.** The coordinator may prepare proposed settings, but should not create the repository or modify other repositories without the required authorization.

Proposed root layout:

```text
web-controller/
├── README.md
├── LICENSE
├── Cargo.toml / Cargo.lock
├── src/
│   ├── main.rs
│   ├── config.rs
│   ├── server/          # HTTP, WS, pairing
│   ├── sessions/        # auth, lifecycle
│   ├── input/           # schema, state, timing
│   ├── controllers/     # owner, Xbox/DualSense/Wii/Steam-Ibex family mappings
│   └── diagnostics.rs
├── web/
│   ├── package.json + lockfile
│   ├── src/             # UI, sensor provider, touch controls
│   └── public/
├── tests/               # contract, integration, fixtures
├── deploy/              # service/udev examples
├── docs/
│   ├── ARCHITECTURE.md
│   ├── PROTOCOL.md
│   ├── SECURITY.md
│   ├── CONTROLLER_TARGETS.md
│   ├── TEST_PLAN.md
│   ├── DECISIONS.md
│   └── adr/
└── .github/             # workflows, issue/PR templates
```

This is an **intended destination layout**, not an instruction to generate empty files. Prefer one Rust crate until concrete reuse/ownership pressures justify additional crates. Keep frontend build outputs untracked unless a deliberate packaging policy requires embedding them.

### Recommended governance

- Establish a protected `main` branch, PR-based changes, mandatory formatter/linter/test checks, and pinned toolchains/dependency lockfiles, appropriate to repository access level.
- Every implementation issue should specify owner, phase, dependencies, acceptance criteria, test evidence, and explicit out-of-scope items. Suggested labels: `area:frontend`, `area:backend`, `area:controller`, `area:security`, `area:deployment`, `area:testing`, `type:research`, `status:blocked`, `needs:decision`.
- Maintain a small decision log/ADRs for changes to the architecture, upstream API target, authentication method, controller presentation, and transport.
- Record upstream `virtualgamepad` commit SHA in the dependency declaration and planning notes. Upgrades require a focused compatibility PR, validation replay, and a new evidence record.
- Separate static/unit tests (must run in standard CI without kernel devices) from opt-in privileged/local Linux integration tests. No blanket assumption that GitHub runners expose `/dev/uinput` or `/dev/uhid`.
- Distinguish **implemented**, **host-enumerated**, and **consumer-validated** status in issue tracking and release notes. Use evidence links, environment/kernel/browser details, and known limitations.
- State security and licensing before repository publication; choose a compatible license through a documented decision rather than inventing one.
- Do not change `virtualgamepad` as part of frontend/service work unless a clearly defined upstream defect is demonstrated. Track upstream fixes as separate issues/PRs with tests and explicit dependency updates.

### Proposed initial issue queue

| ID | Work item | Depends on | Deliverable |
| --- | --- | --- | --- |
| P-01 | Approve repository name/visibility/license/ownership | — | Recorded decision |
| P-02 | Pin reviewed `virtualgamepad` revision and API inventory | P-01 | Dependency contract and limitations |
| P-03 | Approve architecture and module ownership | P-02 | `ARCHITECTURE.md` / ADR |
| P-04 | Specify input protocol, units, and reconnection semantics | P-03 | `PROTOCOL.md` + test vectors |
| P-05 | Define HTTPS pairing, host permission, and threat model | P-03 | `SECURITY.md` |
| P-06 | Define CI, test tiers, and consumer evidence format | P-03 | `TEST_PLAN.md` |
| M1-01 | Build initial service/UI vertical slice | P-04, P-05, P-06 | Xbox uinput demonstration |
| M1-02 | Validate disconnect, teardown, SDL/evdev output | M1-01 | Recorded acceptance evidence |
| M2-01 | Multiple sessions, QR pairing, deployment hardening | M1-02 | Operational service |
| M3-01 | Research and implement browser IMU to DualSense | M2-01 | Consumer motion evidence |
| M4-W01 | Test Wii UHID consumer compatibility before Wii UI | P-02; M3 motion tools useful, not required | Wii feasibility evidence / decision |
| M4-S01 | Audit pinned Ibex upstream API, UHID handshake, SDL and Steam consumer acceptance | P-02; M3 motion tools useful, not required | Ibex feasibility evidence / upstream blocker |
| M5-W01 | Build Wii Remote UI and mapping | M4-W01 success | Wii consumer validation |
| M5-S01 | Build Steam Ibex dual-trackpad + IMU UI and mapping | M4-S01 success | Ibex consumer validation |

**Parallelism:** M4-W01 and M4-S01 can run concurrently. M5-W01 depends only on M4-W01; M5-S01 depends only on M4-S01. Both may leverage M3 IMU instrumentation, but neither should be blocked solely by the other family.

## 8. Technical risk register

| Risk | Impact | Mitigation / gate |
| --- | --- | --- |
| Browser IMU missing, throttled, or inconsistent | DualSense/Wii motion may feel poor | Capability negotiation, measured rates, supported-device matrix, real-game tests |
| HTTPS/certificate friction on local Wi-Fi | Phones cannot enable sensor access | Specify trusted local-domain/certificate workflow before shipping |
| TCP stalls or queued stale frames | Control latency, stuck input | Bounded latest-state ingestion, timestamps, watchdogs, diagnostics; consider WebRTC only with evidence |
| Incorrect IMU axis/unit transforms | Inverted or unusable motion | Declared units/frames, per-orientation calibration, deterministic test vectors |
| Incorrect library servicing schedule | Missing HID output/handshake, apparent freezes | Independent readiness/deadline device worker and idle tests |
| Wii HID recognized differently from real hardware | Dolphin/SDL incompatibility | M4-W consumer feasibility gate before M5-W commitments |
| Ibex API or HID reports unavailable at pinned upstream revision | Cannot create/drive intended Steam personality | Track upstream family readiness, pin tested revision, M4-S gate; do not reimplement HID in service |
| Steam Ibex UHID accepts neutral device but not real controls or Steam-client listing | Misleading compatibility claims | Validate input delivery, handshakes, trackpads, IMU, and Steam listing independently |
| Browser touchscreen cannot match Steam trackpad pressure/haptics | Reduced Steam-controller fidelity | Explicit UX substitutes and per-feature limitations; do not promise hardware parity |
| Untrusted LAN clients can create host input devices | Host abuse/control injection | Pairing authorization, origin checks, limits, unprivileged device access |
| Upstream alpha API changes | Build/behavior regressions | Exact Git pin, lockfile, dedicated upgrade PR, regression suite |
| Permissions/device cleanup | Startup failure or leaked controllers | Preflight, scoped permissions, graceful/failure cleanup tests |
| Licensing obligations | Publication/compliance risk | Review `virtualgamepad` AGPL-3.0-only dependency and distribution implications |

## 9. Planner operating instructions and handback contract

The coordination planner's job is to prepare and maintain **repository state**, not to silently expand scope or implement the controller stack. Its first output should be a concise inventory: whether a new repository exists, authoritative remote URL and default branch (if created), selected upstream commit, agreed decisions, issue dependency graph, current milestone, blockers, and next unblocked task.

When authorized to create the repository, use the accepted name/visibility/license, initialize only agreed governance/docs/scaffolding, and provide verifiable commit/PR references. Do not declare success based on unexecuted setup plans.

During ongoing work, keep `main` consistent, update issue/PR statuses with tested evidence, distinguish proposed from merged and validated changes, and keep an explicit **state-of-repo handback**. Track any changes requiring this architecture to be revisited (for example, failed Wii or Ibex consumer gates, a change to transport, or new virtualgamepad APIs).

**Actual code implementation will be developed in the originating ChatGPT thread.** The planner should open/manage work items and supply a current-state handback to that thread. It should track M4-W/M4-S and M5-W/M5-S independently, and should not autonomously implement M1–M5, rewrite upstream `virtualgamepad`, or declare hardware/application support without a separate instruction and documented validation.

### Required planner handback format

```text
Repository: <approved URL or NOT CREATED>
Default branch / head SHA: <...>
Upstream virtualgamepad pin: <SHA or PENDING>
Approved scope decisions: <...>
Open PRs and relevant issues: <links>
Current milestone / gate: <...>
Verified completed work: <links to tests/evidence>
Blocked / pending decisions: <...>
Next unblocked implementation task: <issue + acceptance criteria>
Changes since previous handback: <...>
```

## 10. Pre-implementation decisions to resolve

1. **Repo identity:** final name, public/private visibility, organization/owner, and license (including AGPL implications).
2. **Upstream baseline:** exact `virtualgamepad` commit and application API to target.
3. **Host deployment model:** user service vs system service and narrowly scoped access to uinput/UHID.
4. **HTTPS trust path:** local hostname/domain and certificate provisioning model that works on iOS and Android.
5. **MVP acceptance environments:** Linux distribution/kernel, primary mobile browsers, and one SDL/game controller test application. For the future Ibex gate, record the host kernel and pinned SDL/hid-steam consumer versions: upstream testing used a Linux 7.3 development host, not a universal kernel compatibility guarantee.
6. **Release criteria:** whether single-phone M1 is an internal prototype or the first published release; M2 security hardening is recommended before general LAN distribution.

**Planning principle:** prove the simplest full browser-to-evdev path first, preserve `virtualgamepad`'s native controller semantics and exact realization selection, and gate every more ambitious motion/HID claim with consumer-level evidence. **Wii MotionPlus and Steam Controller Ibex are parallel advanced-controller targets** with separate acceptance gates and no dependency on one another.
