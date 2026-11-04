import { describeSession, relativeExpiry } from "./admin-model.mjs";

interface SessionView {
  id: number;
  status: string;
  connections: number;
  frames: number;
  expires_in_seconds: number | null;
}

interface InviteView {
  id: number;
  reusable: boolean;
  expires_in_seconds: number | null;
}

interface RequestView {
  id: number;
  approved: boolean;
  expires_in_seconds: number;
}

interface Snapshot {
  exposure_enabled: boolean;
  join_requests_enabled: boolean;
  max_sessions: number;
  sessions: SessionView[];
  invites: InviteView[];
  requests: RequestView[];
}

interface IssuedInvite {
  id: number;
  pin: string;
  url: string;
  reusable: boolean;
}

function requireElement<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`Missing operator element ${selector}`);
  return element;
}

const dashboard = requireElement<HTMLElement>("#dashboard");
const login = requireElement<HTMLElement>("#login");
const form = requireElement<HTMLFormElement>("#login-form");
const feedback = requireElement<HTMLElement>("#feedback");
const connectionStatus = requireElement<HTMLElement>("#connection-status");
const exposed = requireElement<HTMLInputElement>("#exposure");
const joins = requireElement<HTMLInputElement>("#joins");
const capacity = requireElement<HTMLSelectElement>("#capacity");
const invites = requireElement<HTMLUListElement>("#invites");
const sessions = requireElement<HTMLUListElement>("#sessions");
const requests = requireElement<HTMLUListElement>("#requests");
const issued = requireElement<HTMLDivElement>("#issued");
let token = "";
let loading = false;
let settingsDirty = false;
let latest: Snapshot | null = null;

function setFeedback(message: string) {
  feedback.textContent = message;
}

async function api<T>(path: string, body?: object): Promise<T | null> {
  const response = await fetch(path, {
    method: body === undefined ? "GET" : "POST",
    cache: "no-store",
    credentials: "omit",
    headers: {
      Authorization: `Bearer ${token}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  if (response.status === 401) {
    token = "";
    dashboard.classList.add("hidden");
    login.classList.remove("hidden");
    connectionStatus.textContent = "Not authenticated";
    throw new Error("Operator authentication required");
  }
  if (!response.ok) throw new Error(`Request failed (HTTP ${response.status})`);
  return response.status === 204 ? null : ((await response.json()) as T);
}

function button(label: string, action: () => Promise<void>): HTMLButtonElement {
  const item = document.createElement("button");
  item.type = "button";
  item.textContent = label;
  item.addEventListener("click", () => {
    void perform(action);
  });
  return item;
}

function listEntry(
  description: string,
  actions: HTMLButtonElement[] = [],
): HTMLLIElement {
  const element = document.createElement("li");
  const label = document.createElement("span");
  label.textContent = description;
  element.append(label, ...actions);
  return element;
}

function render(snapshot: Snapshot) {
  latest = snapshot;
  if (!settingsDirty) {
    exposed.checked = snapshot.exposure_enabled;
    joins.checked = snapshot.join_requests_enabled;
    capacity.value = String(snapshot.max_sessions);
  }
  connectionStatus.textContent = snapshot.exposure_enabled
    ? `Live · ${snapshot.sessions.length} authorized`
    : "Exposure disabled";

  invites.replaceChildren(
    ...snapshot.invites.map((invite) =>
      listEntry(
        `Invite ${invite.id} · ${
          invite.reusable ? "reusable" : relativeExpiry(invite.expires_in_seconds)
        }`,
        [
          button("Revoke", () =>
            command(`/api/invites/${invite.id}/revoke`, {}),
          ),
        ],
      ),
    ),
  );
  sessions.replaceChildren(
    ...snapshot.sessions.map((session) =>
      listEntry(
        describeSession(session),
        [
          button("Kick", async () => {
            if (!window.confirm(`Kick controller ${session.id}?`)) return;
            await command(`/api/sessions/${session.id}/kick`, {});
          }),
        ],
      ),
    ),
  );
  requests.replaceChildren(
    ...snapshot.requests.map((request) =>
      listEntry(
        `Request ${request.id} · ${
          request.approved ? "approved" : "pending"
        } · ${relativeExpiry(request.expires_in_seconds)}`,
        [
          button("Approve", () =>
            command(`/api/requests/${request.id}/resolve`, { approved: true }),
          ),
          button("Reject", () =>
            command(`/api/requests/${request.id}/resolve`, { approved: false }),
          ),
        ],
      ),
    ),
  );
}

async function refresh() {
  if (!token || loading) return;
  loading = true;
  try {
    const snapshot = await api<Snapshot>("/api/state");
    if (snapshot) render(snapshot);
  } catch (error) {
    const message = error instanceof Error ? error.message : "Refresh failed";
    setFeedback(message);
  } finally {
    loading = false;
  }
}

async function command(path: string, body: object) {
  await api(path, body);
  setFeedback("Change applied");
  await refresh();
}

async function perform(action: () => Promise<void>) {
  try {
    await action();
  } catch (error) {
    setFeedback(error instanceof Error ? error.message : "Action failed");
  }
}

form.addEventListener("submit", (event) => {
  event.preventDefault();
  const input = requireElement<HTMLInputElement>("#operator-token");
  token = input.value;
  input.value = "";
  void perform(async () => {
    const snapshot = await api<Snapshot>("/api/state");
    if (!snapshot) throw new Error("Empty operator state");
    login.classList.add("hidden");
    dashboard.classList.remove("hidden");
    setFeedback("");
    render(snapshot);
  });
});

requireElement<HTMLFormElement>("#settings-form").addEventListener(
  "change",
  () => {
    settingsDirty = true;
  },
);

requireElement<HTMLFormElement>("#settings-form").addEventListener(
  "submit",
  (event) => {
  event.preventDefault();
  void perform(async () => {
    if (
      latest?.exposure_enabled &&
      !exposed.checked &&
      !window.confirm("Disable exposure and disconnect all active controllers?")
    ) {
      settingsDirty = false;
      render(latest);
      return;
    }
    await command("/api/settings", {
      exposure_enabled: exposed.checked,
      join_requests_enabled: joins.checked,
      max_sessions: Number(capacity.value),
    });
    settingsDirty = false;
  },
);

requireElement<HTMLButtonElement>("#issue").addEventListener("click", () => {
  void perform(async () => {
    const reusable = requireElement<HTMLInputElement>("#reusable").checked;
    const response = await api<IssuedInvite>("/api/invites", { reusable });
    if (!response) throw new Error("Invite creation failed");
    issued.replaceChildren();
    const pin = document.createElement("p");
    pin.textContent = `New invite ${response.id}: PIN ${response.pin}`;
    const link = document.createElement("input");
    link.type = "text";
    link.readOnly = true;
    link.value = response.url;
    link.setAttribute("aria-label", "Controller invitation link");
    const copy = button("Copy invitation link", async () => {
      await navigator.clipboard.writeText(response.url);
      setFeedback("Invitation copied");
    });
    issued.append(pin, link, copy);
    await refresh();
  });
});

window.setInterval(() => {
  void refresh();
}, 1500);
