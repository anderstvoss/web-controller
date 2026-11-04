export function relativeExpiry(seconds: number | null): string {
  return seconds === null ? "until revoked" : `expires in ${seconds}s`;
}

export function describeSession(session: {
  id: number;
  status: string;
  connections: number;
  frames: number;
  expires_in_seconds: number | null;
}): string {
  const expiry =
    session.expires_in_seconds === null
      ? "no expiration"
      : relativeExpiry(session.expires_in_seconds);
  return `Controller ${session.id} · ${session.status} · ${session.connections} connections · ${session.frames} frames · ${expiry}`;
}
