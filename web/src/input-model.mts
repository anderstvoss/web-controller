// M1 full-state protocol model; also tested under Node's built-in test runner.
export const BUTTON_COUNT = 15;

export interface InputState {
  buttons: boolean[];
  sticks: [number, number, number, number];
  triggers: [number, number];
}

export function newState(): InputState {
  return {
    buttons: Array.from({ length: BUTTON_COUNT }, () => false),
    sticks: [0, 0, 0, 0],
    triggers: [0, 0],
  };
}

export function snapshot(state: InputState, seq: number, clientMs: number) {
  return {
    v: 1 as const,
    seq,
    client_ms: clientMs,
    buttons: [...state.buttons],
    sticks: [...state.sticks],
    triggers: [...state.triggers],
  };
}

export function setStick(
  state: InputState,
  index: 0 | 1,
  dx: number,
  dy: number,
  radius: number,
) {
  if (!Number.isFinite(dx) || !Number.isFinite(dy) || !(radius > 0)) {
    throw new Error("invalid stick geometry");
  }
  const length = Math.hypot(dx, dy);
  const scale = length > radius ? radius / length : 1;
  // Positive Y is physically upwards; browser pointer coordinates increase downwards.
  state.sticks[index * 2] = Math.max(-1, Math.min(1, (dx * scale) / radius));
  state.sticks[index * 2 + 1] = Math.max(
    -1,
    Math.min(1, -(dy * scale) / radius),
  );
}

export function clearStick(state: InputState, index: 0 | 1) {
  state.sticks[index * 2] = 0;
  state.sticks[index * 2 + 1] = 0;
}

export function neutralize(state: InputState) {
  state.buttons.fill(false);
  state.sticks.fill(0);
  state.triggers.fill(0);
}
