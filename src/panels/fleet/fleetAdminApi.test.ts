import { afterEach, describe, expect, test } from "bun:test";
import {
  getClipboardRules, loginFleetAdmin, saveInkPolicy, updateOrgSettings,
} from "./fleetAdminApi";
import type { FleetSession, InkReceiptPolicy } from "./fleetAdminTypes";

const realFetch = globalThis.fetch;
afterEach(() => { globalThis.fetch = realFetch; });

const session: FleetSession = {
  serverUrl: "http://localhost:8787",
  orgId: "local",
  email: "admin@example.test",
  role: "admin",
  token: "memory-token",
  expiresAt: "2030-01-01T00:00:00Z",
};

function mockJson(body: unknown) {
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  globalThis.fetch = (async (url: string | URL | Request, init?: RequestInit) => {
    calls.push({ url: String(url), init });
    return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
  }) as typeof fetch;
  return calls;
}

describe("native Fleet admin API client", () => {
  test("logs in without persisting credentials and trims the server URL", async () => {
    const calls = mockJson({ token: "t", role: "admin", expires_at: "2030-01-01T00:00:00Z" });
    const result = await loginFleetAdmin("http://localhost:8787/", "local", "admin@example.test", "secret");
    expect(result.serverUrl).toBe("http://localhost:8787");
    expect(calls[0].url).toBe("http://localhost:8787/api/v1/auth/login");
    expect(JSON.parse(String(calls[0].init?.body))).toEqual({ email: "admin@example.test", password: "secret" });
  });

  test("uses bearer auth and the selected organization for policy calls", async () => {
    const calls = mockJson([]);
    await getClipboardRules(session);
    expect(calls[0].url.endsWith("/api/v1/orgs/local/clipboard-guard/rules")).toBe(true);
    expect(new Headers(calls[0].init?.headers).get("Authorization")).toBe("Bearer memory-token");
  });

  test("sends server field casing for settings and camel casing for Ink policy", async () => {
    const settingsCalls = mockJson({ clipboard_guard_enabled: true });
    await updateOrgSettings(session, { clipboard_guard_enabled: true });
    expect(JSON.parse(String(settingsCalls[0].init?.body))).toEqual({ clipboard_guard_enabled: true });

    const policy: InkReceiptPolicy = {
      managedDestinations: [{ name: "PDF", printerClass: "pdf" }],
      ticketRequired: true,
      offlineBehavior: "ex_post_duplicate_detection",
      watermarkTemplate: "{ticket}",
      failureStance: { pdf: "fail_closed", securePhysical: "fail_soft" },
    };
    const inkCalls = mockJson({ ...policy, policy_version: 1 });
    await saveInkPolicy(session, policy);
    expect(JSON.parse(String(inkCalls[0].init?.body))).toEqual(policy);
  });
});
