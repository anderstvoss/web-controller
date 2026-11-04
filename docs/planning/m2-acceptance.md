# M2 two-phone acceptance

Record the tested commit, Linux distribution/kernel, both phone/browser
versions, SDL build, and whether each browser accepted the HTTPS certificate
without a warning. Run the test on a trusted LAN with two phones that can
reach the reverse proxy. Confirm the service user can write to `/dev/uinput`
and read the created evdev devices for inspection. Keep the service terminal
and an evdev or SDL input tester open on the host.

## Browser and touch acceptance

Use two awake phones for the controls and UI checks. Network lifecycle is
covered independently by automated local clients.

| Step | Expected observation |
| --- | --- |
| Pair phone A by QR and phone B by PIN, then exercise host-approved joining | Each route reaches its own controller page. The host approval label matches the requesting browser. |
| Hold A on one phone and move the other phone's right stick, then release | Each evdev/SDL device shows only its own phone's input and returns to neutral. Check the remaining controls, multi-touch, pointer cancellation, and visibility releases as browser behavior. |
| Repeat the pairing and input smoke test after migrating the frontend | The built TypeScript assets preserve the pairing and touch behavior. |

## Automated transport and reconnect acceptance

`cargo test --locked --all-features` includes local HTTP/WebSocket clients
against the real application router with an observable fake device owner.
These tests need no physical phones, browser, TLS provisioning or kernel devices.

The transport scenario covers two independent sessions, abrupt TCP loss
without a WebSocket close frame, silent-transport timeout, reconnect within
grace with a fresh sequence counter, grace expiry and capacity release,
reusable reauthorization, sequence replay, rate-limit closure, failed device
startup, explicit end and shutdown cleanup. A heartbeat on the second client
continues while the first disconnects and expires.

Separate opt-in synthetic host probes exercise the real unprivileged uinput
backend and SDL. Keep that evidence distinct from browser touch observations.

Use `list` for host session IDs, connection/frame counts and remaining grace.
Check HTTPS trust and origin configuration if the page loads but pairing or
WSS fails. If pairing succeeds but no device appears, check uinput access
and the terminal's device error. If QR rendering is unavailable, check
`qrencode` on the host or use the PIN. During reconnection keep the same
browser profile so it can present its session cookie.

Attach pass/fail observations to the PR. Keep certificate files, addresses,
credentials and raw traffic out of Git. Synthetic tests and enumeration
support their own evidence boundaries. Record two-phone consumer validation
only after the live test above.
