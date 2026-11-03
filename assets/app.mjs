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

function setStatus(message) {
  status.textContent = message;
}

function sendSnapshot() {
  if (!socket || socket.readyState !== WebSocket.OPEN || socket.bufferedAmount > 1024) {
    return;
  }
  sequence += 1;
  socket.send(JSON.stringify(snapshot(state, sequence)));
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
  if (!socket || socket.readyState !== WebSocket.OPEN) {
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

function connect() {
  const uri = new URL("/input", window.location.href);
  uri.protocol = "wss:";
  socket = new WebSocket(uri);
  socket.addEventListener("open", () => {
    setStatus("Controller connected");
    pairView.classList.add("hidden");
    controller.classList.remove("hidden");
    sequence = 0;
    intervalId = window.setInterval(sendSnapshot, 1000 / 60);
    sendSnapshot();
  });
  socket.addEventListener("message", () => {
    setStatus("Host failed to create controller");
    socket.close();
  });
  socket.addEventListener("close", () => {
    window.clearInterval(intervalId);
    resetControls();
    pairView.classList.remove("hidden");
    controller.classList.add("hidden");
    setStatus("Disconnected");
  });
  socket.addEventListener("error", () => setStatus("Network error"));
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  errorText.textContent = "";
  const codeInput = document.querySelector("#pair-code");
  const code = codeInput.value;
  try {
    const response = await fetch("/pair", {
      method: "POST",
      credentials: "same-origin",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code }),
    });
    if (!response.ok) {
      throw new Error(`Pairing rejected (HTTP ${response.status})`);
    }
    codeInput.value = "";
    setStatus("Connecting");
    connect();
  } catch (error) {
    errorText.textContent = error instanceof Error ? error.message : "Pairing failed";
  }
});

document.addEventListener("visibilitychange", () => {
  if (document.hidden) resetControls();
});
window.addEventListener("blur", resetControls);
window.addEventListener("pagehide", resetControls);
