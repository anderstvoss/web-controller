import { describe, expect, it } from "vitest";
import { describeSession, relativeExpiry } from "../src/admin-model.mjs";

describe("operator display contract", () => {
  it("identifies runtime-only invite expiry", () => {
    expect(relativeExpiry(null)).toBe("until revoked");
    expect(relativeExpiry(45)).toBe("expires in 45s");
  });

  it("shows session state without any bearer token", () => {
    const result = describeSession({
      id: 7,
      status: "connected",
      connections: 2,
      frames: 52,
      expires_in_seconds: null,
    });
    expect(result).toContain("Controller 7");
    expect(result).toContain("2 connections");
    expect(result).toContain("52 frames");
  });
});
