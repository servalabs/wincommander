import type {
  ClipboardRule,
  FleetOrgSettings,
  FleetRole,
  FleetSession,
  InkReceiptPolicy,
  InkReceiptPolicyView,
} from "./fleetAdminTypes";

const trimUrl = (url: string) => url.trim().replace(/\/+$/, "");

async function parseResponse<T>(response: Response): Promise<T> {
  const body = await response.json().catch(() => null) as T | { error?: string } | null;
  if (!response.ok) {
    const detail = body && typeof body === "object" && "error" in body ? body.error : null;
    throw new Error(detail || `Fleet server returned HTTP ${response.status}`);
  }
  return body as T;
}

async function request<T>(
  session: Pick<FleetSession, "serverUrl" | "token">,
  path: string,
  init?: RequestInit,
): Promise<T> {
  return parseResponse<T>(await fetch(`${trimUrl(session.serverUrl)}${path}`, {
    ...init,
    headers: {
      Accept: "application/json",
      Authorization: `Bearer ${session.token}`,
      ...(init?.body ? { "Content-Type": "application/json" } : {}),
      ...init?.headers,
    },
  }));
}

export async function loginFleetAdmin(
  serverUrl: string,
  orgId: string,
  email: string,
  password: string,
): Promise<FleetSession> {
  const response = await parseResponse<{ token: string; role: FleetRole; expires_at: string }>(
    await fetch(`${trimUrl(serverUrl)}/api/v1/auth/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify({ email: email.trim(), password }),
    }),
  );
  return {
    serverUrl: trimUrl(serverUrl),
    orgId: orgId.trim() || "local",
    email: email.trim(),
    role: response.role,
    token: response.token,
    expiresAt: response.expires_at,
  };
}

const orgPath = (session: FleetSession, tail: string) =>
  `/api/v1/orgs/${encodeURIComponent(session.orgId)}${tail}`;

export const getOrgSettings = (session: FleetSession) =>
  request<FleetOrgSettings>(session, orgPath(session, "/settings"));

export const updateOrgSettings = (session: FleetSession, patch: Partial<FleetOrgSettings>) =>
  request<FleetOrgSettings>(session, orgPath(session, "/settings"), {
    method: "PUT",
    body: JSON.stringify(patch),
  });

export const getClipboardRules = (session: FleetSession) =>
  request<ClipboardRule[]>(session, orgPath(session, "/clipboard-guard/rules"));

export const saveClipboardRules = (session: FleetSession, rules: ClipboardRule[]) =>
  request<ClipboardRule[]>(session, orgPath(session, "/clipboard-guard/rules"), {
    method: "PUT",
    body: JSON.stringify({ rules }),
  });

export const testClipboardRules = (session: FleetSession, rules: ClipboardRule[], text: string) =>
  request<{ compiled: boolean; errors: string[]; verdict: unknown }>(
    session,
    orgPath(session, "/clipboard-guard/rules/test"),
    { method: "POST", body: JSON.stringify({ rules, text }) },
  );

export const publishClipboardRules = (session: FleetSession) =>
  request<{ version: number }>(session, orgPath(session, "/clipboard-guard/publish"), {
    method: "POST",
  });

export const getInkPolicy = (session: FleetSession) =>
  request<InkReceiptPolicyView | null>(session, orgPath(session, "/ink-receipt/policy"));

export const saveInkPolicy = (session: FleetSession, policy: InkReceiptPolicy) =>
  request<InkReceiptPolicyView>(session, orgPath(session, "/ink-receipt/policy"), {
    method: "PUT",
    body: JSON.stringify(policy),
  });
