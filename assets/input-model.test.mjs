import { test } from "node:test";
import assert from "node:assert/strict";
import { newState, snapshot, setStick, clearStick, neutralize } from "./input-model.mjs";

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
