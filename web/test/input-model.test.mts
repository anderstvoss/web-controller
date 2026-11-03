import fc from "fast-check";
import { test } from "vitest";
import assert from "node:assert/strict";
import {
  newState,
  snapshot,
  setStick,
  clearStick,
  neutralize,
} from "../src/input-model.mjs";

test("M1 state has the exact advertised shape", () => {
  const value = snapshot(newState(), 5, 42.25);
  assert.equal(value.v, 1);
  assert.equal(value.seq, 5);
  assert.equal(value.client_ms, 42.25);
  assert.equal(value.buttons.length, 15);
  assert.deepEqual(value.sticks, [0, 0, 0, 0]);
  assert.deepEqual(value.triggers, [0, 0]);
});

test("stick axes clamp to the unit circle and invert browser Y", () => {
  const value = newState();
  setStick(value, 0, 100, -100, 100);
  assert.ok(value.sticks[0] > 0);
  assert.ok(value.sticks[1] > 0);
  assert.ok(Math.hypot(value.sticks[0], value.sticks[1]) <= 1.000001);
  clearStick(value, 0);
  assert.deepEqual(value.sticks, [0, 0, 0, 0]);
});

test("neutralization clears all held controls", () => {
  const value = newState();
  value.buttons[0] = true;
  value.triggers[1] = 1;
  setStick(value, 1, 30, 40, 50);
  neutralize(value);
  assert.deepEqual(value, newState());
});

test("snapshot does not alias mutable controller state", () => {
  const value = newState();
  const saved = snapshot(value, 1, 22.5);
  value.buttons[0] = true;
  assert.equal(saved.buttons[0], false);
});

test("encoded frames fit the protocol and preserve normalized axes", () => {
  fc.assert(
    fc.property(
      fc.integer({ min: 0, max: Number.MAX_SAFE_INTEGER }),
      fc.integer({ min: -1000, max: 1000 }),
      fc.integer({ min: -1000, max: 1000 }),
      fc.array(fc.boolean(), { minLength: 15, maxLength: 15 }),
      (seq, x, y, buttons) => {
        const input = newState();
        input.buttons = buttons;
        setStick(input, 0, x, y, 100);
        setStick(input, 1, -x, -y, 100);
        input.triggers = [1, 0];
        const frame = snapshot(input, seq, 123.5);
        const encoded = JSON.stringify(frame);
        assert.ok(Buffer.byteLength(encoded) <= 512);
        assert.equal(JSON.stringify(JSON.parse(encoded)), encoded);
        assert.ok(
          Math.hypot(frame.sticks[0] ?? 0, frame.sticks[1] ?? 0) <= 1.000001,
        );
        assert.ok(
          Math.hypot(frame.sticks[2] ?? 0, frame.sticks[3] ?? 0) <= 1.000001,
        );
      },
    ),
  );
});
