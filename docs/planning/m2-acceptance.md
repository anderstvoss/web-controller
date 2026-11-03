# M2 two-phone acceptance

Record the tested commit, Linux distribution/kernel, both phone/browser
versions, SDL build, and whether each browser accepted the HTTPS certificate
without a warning. Run the test on a trusted LAN with two phones that can
reach the reverse proxy. Confirm the service user can write to `/dev/uinput`
and read the created evdev devices for inspection. Keep the service terminal
and an evdev or SDL input tester open on the host.

| Step | Expected observation |
| --- | --- |
| Generate two invites and pair phone A by QR and phone B by PIN | Two devices appear. Attempt to reuse each invite's other credential and confirm it is rejected. |
| Hold A on one phone and move a stick on the other, then swap | Each tester shows only its own phone's input. Check both stick signs, D-pad, buttons, triggers, multi-touch and releases. |
| Interrupt phone A's network while a control is held, then restore it promptly | The device neutralizes, the browser reconnects within 15 seconds, and the same evdev node resumes. Phone B keeps working. |
| Disconnect A again and wait beyond 15 seconds | A's node disappears and reconnect requires new authorization. B stays connected. |
| Open the reusable URL on A and request to join | The phone shows a request ID. Approve that ID with the terminal. A gets a fresh device. |
| End A from the browser and B from the terminal | Both nodes disappear. Repeat a pairing and use `quit` to verify shutdown teardown. |

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
