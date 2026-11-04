# M2.5 — Live management console

**State:** implementation candidate; host validation required. **Branch:** `feat/m2-5-live-admin-console`, stacked on M2 TypeScript PR #8.

## Purpose

Add a **separate local operator control plane** to the standalone web-controller service, without mixing management privileges into the phone's publicly reachable controller page. Preserve M2's session registry as the single owner of invitation, session, and virtual-controller lifetime. The console is a live, browser-accessible replacement for common stdin commands; stdin remains available.

## Functional contract

- Expose an opt-in administration HTTP listener on a **different port** from `WEB_CONTROLLER_BIND`. It is **loopback-only**; remote administration is by an independently authenticated SSH port forward, not public LAN exposure.
- Require an operator-supplied high-entropy token (minimum 32 characters) in an `Authorization: Bearer` header on **every API request**. Keep the token in browser memory, never in URLs, cookies, localStorage or log output. Require same-origin requests on mutations; no CORS and no controller-session cookie recognized as admin credentials.
- Live status polling (~1s) displays exposure, pending joins, issued invitations and session status, connections/frames, pending expiry. Never return session bearer tokens or issued invitation secrets in a status/list response.
- Toggle controller exposure: disabling blocks new invitations, pairing, joining and controller attachments, revokes invites/requests, and terminates active sessions and their `virtualgamepad` devices. Enabling resumes admissions but **does not automatically recreate devices**.
- Configure join-request availability and maximum permitted concurrently authorized sessions (1–4). A reduction does not evict existing sessions; capacity applies to subsequent authorizations.
- Issue one-time or reusable invitations from the GUI (with PIN and URL returned **only on creation**), revoke invitations, approve/reject join requests, and kick/terminate individual sessions.
- The GUI's **Create controller invitation** creates an enrollment invitation, not an unattached virtual device. In M2 a virtual controller is instantiated **when a phone claims a session and attaches its WebSocket**. Avoid claiming otherwise.
- Runtime-only configuration (no secrets/settings file and no new privileged broker). Upon restart, exposure and requests return to enabled and capacity to the existing default, unless future settings persistence is separately approved.

## Boundaries and security

```text
Phone: HTTPS origin → player listener → Sessions → SocketLease → virtualgamepad
                                                ▲
Operator: loopback separate port → admin router ┘
                 Bearer auth + Origin check
```

Use only a single `Sessions` instance shared by both listeners. All registry changes happen under its existing mutex; never construct/close HID handles from administration request handlers. Retain stop/expiry/neutralization semantics. Stopping the primary service also stops its admin listener.

The console's static HTML/TypeScript/CSS is bundled locally with M2's frontend build; it has a restrictive CSP, no external script/CDN dependencies, text-only DOM rendering (no `innerHTML`), and `Cache-Control: no-store`. Admin binding must fail closed for non-loopback addresses, port collisions, absent secret, malformed origin, and missing built assets. The admin port has **no trusted HTTPS certificate by default**: loopback HTTP is acceptable only because it cannot be reached from another host. For remote operations, use a secure SSH tunnel and configure the browser origin accordingly. Never place the management port behind the public player reverse proxy.

## Acceptance

1. Unit tests for exposure disable/re-enable, admission denial, invite/request invalidation, session kick and remaining device cleanup, capacity and join-request policy.
2. Handler tests for forbidden unauthenticated GET/POST, missing/wrong Origin, invalid JSON/config and valid authorized actions. Prefer deterministic fake owners and no kernel dependencies.
3. Frontend strict TS/lint/format/test + fake API response rendering/actions.
4. Both listeners bind, serve different UIs and shut down together; admin listener rejects non-loopback binds and same port; no extra device protocol implementations.
5. Host operator manually verifies the second port, PIN issuance, approved join, kick, exposure-disable destruction/neutralization and reopening; record host environment separately. CI alone does not prove controller consumer behavior.
6. `AGENTS.md` full Rust, dependency, frontend and secret-scanning gates pass. No change to CI/policy files or upstream `virtualgamepad`.

## Future work (not included)

- Persistent operator policy/audit trail, secure remote admin with mutual TLS, role-based accounts, multiple administrator sessions.
- True pre-created/reserved virtual devices independent of phone sessions, arbitrary controller families/realization selection, and cross-host deployments.
- SSE/WebSocket push updates; polling is sufficient for this bounded M2.5 scope.

M2.5 depends on **both M2 PR #7 and PR #8**, and remains independent of the later DualSense (M3), Wii MotionPlus, and Steam Ibex work.
