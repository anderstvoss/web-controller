// M1 full-state protocol model; also tested under Node's built-in test runner.
export const BUTTON_COUNT = 15;

export function newState() {
  return {
    buttons: Array(BUTTON_COUNT).fill(false),
    sticks: [0, 0, 0, 0],
    triggers: [0, 0],
  };
}

export function snapshot(state, seq) {
  return {
    v: 1,
    seq,
    buttons: [...state.buttons],
    sticks: [...state.sticks],
    triggers: [...state.triggers],
  };
}

export function setStick(state, index, dx, dy, radius) {
  if (!Number.isFinite(dx) || !Number.isFinite(dy) || !(radius > 0)) {
    throw new Error("invalid stick geometry");
  }
  const length = Math.hypot(dx, dy);
  const scale = length > radius ? radius / length : 1;
  // Positive Y is physically upwards; browser pointer coordinates increase downwards.
  state.sticks[index * 2] = Math.max(-1, Math.min(1, (dx * scale) / radius));
  state.sticks[index * 2 + 1] = Math.max(-1, Math.min(1, -(dy * scale) / radius));
}

export function clearStick(state, index) {
  state.sticks[index * 2] = 0;
  state.sticks[index * 2 + 1] = 0;
}

export function neutralize(state) {
  state.buttons.fill(false);
  state.sticks.fill(0);
  state.triggers.fill(0);
}
