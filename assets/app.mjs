import { newState, snapshot, setStick, clearStick, neutralize } from "/input-model.mjs";

const state = newState();
const activePointers = new Map();
const status = document.querySelector("#status");
const pairView = document.querySelector("#pair-view");
const controller = document.querySelector("#controller");
const form = document.querySelector("#pair-form");
const errorText = document.querySelector("#pair-error");
let socket = null;
let intervalId = null;
let sequence = 0;
let ready = false;
let connecting = false;
let reconnectUntil = null;
let retryTimer;
const joinButton = document.querySelector("#request-join");
const endButton = document.querySelector("#end-session");

function setStatus(message) {
  status.textContent = message;
}

function sendSnapshot() {
  if (!ready || !socket || socket.readyState !== WebSocket.OPEN || socket.bufferedAmount > 1024) {
    return;
  }
  sequence += 1;
  socket.send(JSON.stringify(snapshot(state, sequence, performance.now())));
}

function resetControls() {
  activePointers.clear();
  neutralize(state);
  document.querySelectorAll(".pressed").forEach((node) => node.classList.remove("pressed"));
  document.querySelectorAll(".stick-knob").forEach((node) => {
    node.style.transform = "";
  });
  sendSnapshot();
}

function updateButton(element, pressed) {
  if (element.dataset.button !== undefined) {
    const index = Number(element.dataset.button);
    state.buttons[index] = pressed;
  } else if (element.dataset.trigger !== undefined) {
    const index = Number(element.dataset.trigger);
    state.triggers[index] = pressed ? 1 : 0;
  }
  element.classList.toggle("pressed", pressed);
  sendSnapshot();
}

function updateStick(element, event) {
  const index = Number(element.dataset.stick);
  const rect = element.getBoundingClientRect();
  const radius = Math.min(rect.width, rect.height) / 2;
  const dx = event.clientX - (rect.left + rect.width / 2);
  const dy = event.clientY - (rect.top + rect.height / 2);
  setStick(state, index, dx, dy, radius);
  const knob = element.querySelector(".stick-knob");
  const travel = radius * 0.45;
  knob.style.transform = `translate(${state.sticks[index * 2] * travel}px, ${-state.sticks[index * 2 + 1] * travel}px)`;
  sendSnapshot();
}

function onPointerDown(event) {
  if (!ready || !socket || socket.readyState !== WebSocket.OPEN) {
    return;
  }
  const element = event.currentTarget;
  if (element.dataset.stick !== undefined) {
    const index = Number(element.dataset.stick);
    for (const assigned of activePointers.values()) {
      if (assigned.element.dataset.stick !== undefined &&
          Number(assigned.element.dataset.stick) === index) {
        return;
      }
    }
  }
  event.preventDefault();
  element.setPointerCapture(event.pointerId);
  activePointers.set(event.pointerId, { element });
  if (element.dataset.stick !== undefined) {
    updateStick(element, event);
  } else {
    updateButton(element, true);
  }
}

function onPointerMove(event) {
  const assigned = activePointers.get(event.pointerId);
  if (assigned?.element === event.currentTarget &&
      assigned.element.dataset.stick !== undefined) {
    event.preventDefault();
    updateStick(assigned.element, event);
  }
}

function onPointerEnd(event) {
  const assigned = activePointers.get(event.pointerId);
  if (!assigned || assigned.element !== event.currentTarget) {
    return;
  }
  activePointers.delete(event.pointerId);
  const element = assigned.element;
  if (element.dataset.stick !== undefined) {
    const index = Number(element.dataset.stick);
    clearStick(state, index);
    element.querySelector(".stick-knob").style.transform = "";
    sendSnapshot();
  } else {
    // Several fingers may hold the same control; release only after the last.
    const stillPressed = [...activePointers.values()].some((item) => item.element === element);
    updateButton(element, stillPressed);
  }
}

for (const element of document.querySelectorAll("[data-button], [data-trigger], [data-stick]")) {
  element.addEventListener("pointerdown", onPointerDown);
  element.addEventListener("pointermove", onPointerMove);
  element.addEventListener("pointerup", onPointerEnd);
  element.addEventListener("pointercancel", onPointerEnd);
  element.addEventListener("lostpointercapture", onPointerEnd);
  element.addEventListener("contextmenu", (event) => event.preventDefault());
}

function showPairing(message) {
  ready = false;
  pairView.classList.remove("hidden");
  controller.classList.add("hidden");
  setStatus(message);
}

function connect() {
  if (socket) return;
  const uri = new URL("/input", window.location.href);
  uri.protocol = "wss:";
  const connection = new WebSocket(uri);
  socket = connection;
  connection.addEventListener("message", ({ data }) => {
    if (data !== "ready") {
      errorText.textContent = "Host failed to create controller";
      return;
    }
    ready = true;
    reconnectUntil = null;
    sequence = 0;
    setStatus("Controller connected");
    pairView.classList.add("hidden");
    controller.classList.remove("hidden");
    intervalId = window.setInterval(sendSnapshot, 1000 / 60);
    sendSnapshot();
  });
  connection.addEventListener("close", () => {
    socket = null;
    ready = false;
    window.clearInterval(intervalId);
    resetControls();
    if (reconnectUntil === null) reconnectUntil = performance.now() + 15000;
    showPairing("Reconnecting…");
    retryTimer = window.setTimeout(resume, 500);
  });
}

async function post(path, body) {
  return fetch(path, {
    method: "POST",
    credentials: "same-origin",
    cache: "no-store",
    ...(body === undefined ? {} : {
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }),
  });
}

async function resume() {
  if (connecting || socket) return;
  connecting = true;
  try {
    const response = await post("/session");
    if (response.status === 204) {
      if (reconnectUntil === null || performance.now() < reconnectUntil) {
        connect();
        return;
      }
    } else if (response.status === 401) {
      reconnectUntil = null;
      showPairing("Ready to pair");
      return;
    }
  } catch {
    // Retry briefly when Wi-Fi returns.
  } finally {
    connecting = false;
  }
  if (reconnectUntil !== null && performance.now() < reconnectUntil) {
    retryTimer = window.setTimeout(resume, 500);
  } else {
    reconnectUntil = null;
    showPairing("Disconnected");
  }
}

async function pair(code) {
  const response = await post("/pair", { code });
  if (response.status !== 204) throw new Error(`Pairing rejected (HTTP ${response.status})`);
}

async function pairAction(action) {
  if (connecting || socket) return;
  window.clearTimeout(retryTimer);
  reconnectUntil = null;
  connecting = true;
  errorText.textContent = "";
  try {
    await action();
    connect();
  } catch (error) {
    errorText.textContent = error instanceof Error ? error.message : "Pairing failed";
  } finally {
    connecting = false;
  }
}

form.addEventListener("submit", (event) => {
  event.preventDefault();
  const input = document.querySelector("#pair-code");
  void pairAction(async () => {
    await pair(input.value);
    input.value = "";
  });
});

joinButton.addEventListener("click", () => {
  void pairAction(async () => {
    const request = await post("/join/request");
    if (request.status !== 202) throw new Error("Host cannot accept more join requests");
    setStatus(`Ask the host to approve request ${await request.text()}`);
    const deadline = performance.now() + 120000;
    while (performance.now() < deadline) {
      await new Promise((resolve) => window.setTimeout(resolve, 1000));
      const result = await post("/join/claim");
      if (result.status === 204) return;
      if (result.status === 401) break;
      if (result.status !== 202 && result.status !== 409) break;
    }
    showPairing("Ready to pair");
    throw new Error("Join request expired or was rejected");
  });
});

endButton.addEventListener("click", async () => {
  try {
    const response = await post("/session/end");
    if (response.status !== 204) throw new Error("End failed");
    socket?.close();
  } catch {
    setStatus("Could not end the session");
  }
});

const invite = new URLSearchParams(window.location.hash.slice(1)).get("invite");
if (window.location.hash) window.history.replaceState(null, "", window.location.pathname);
if (invite) {
  void pairAction(() => pair(invite));
} else {
  void resume();
}

document.addEventListener("visibilitychange", () => {
  if (document.hidden) resetControls();
});
window.addEventListener("blur", resetControls);
window.addEventListener("pagehide", resetControls);
