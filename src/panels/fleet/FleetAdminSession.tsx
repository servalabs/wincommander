import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { loginFleetAdmin } from "./fleetAdminApi";
import type { FleetSession } from "./fleetAdminTypes";

interface SessionContextValue {
  session: FleetSession | null;
  signIn: (session: FleetSession) => void;
  signOut: () => void;
}

const FleetSessionContext = createContext<SessionContextValue | null>(null);

export function useFleetSession() {
  const value = useContext(FleetSessionContext);
  if (!value) throw new Error("useFleetSession must be used inside FleetAdminSession");
  return value;
}

export default function FleetAdminSession({ children }: { children: ReactNode }) {
  const [session, setSession] = useState<FleetSession | null>(null);
  const context = useMemo(() => ({
    session,
    signIn: (next: FleetSession) => setSession(next),
    signOut: () => setSession(null),
  }), [session]);
  return <FleetSessionContext.Provider value={context}>{children}</FleetSessionContext.Provider>;
}

export function FleetAdminGate({ children }: { children: ReactNode }) {
  const context = useFleetSession();
  const { session } = context;
  const [serverUrl, setServerUrl] = useState("http://localhost:8787");
  const [orgId, setOrgId] = useState("local");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  if (!session) {
    const submit = async (event: React.FormEvent) => {
      event.preventDefault();
      setBusy(true);
      setError("");
      try {
        const next = await loginFleetAdmin(serverUrl, orgId, email, password);
        context.signIn(next);
        setPassword("");
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        setBusy(false);
      }
    };

    return (
      <form className="fleet-admin-login" onSubmit={submit}>
        <div>
          <h3>Fleet administrator sign-in</h3>
          <p>Authenticate to edit organization policy. The token and password stay in memory.</p>
        </div>
        <div className="fleet-form-grid">
          <label><Label htmlFor="fleet-server">Server URL</Label><Input id="fleet-server" value={serverUrl} onChange={e => setServerUrl(e.target.value)} required /></label>
          <label><Label htmlFor="fleet-org">Organization ID</Label><Input id="fleet-org" value={orgId} onChange={e => setOrgId(e.target.value)} required /></label>
          <label><Label htmlFor="fleet-email">Admin email</Label><Input id="fleet-email" type="email" value={email} onChange={e => setEmail(e.target.value)} required /></label>
          <label><Label htmlFor="fleet-password">Password</Label><Input id="fleet-password" type="password" value={password} onChange={e => setPassword(e.target.value)} required /></label>
        </div>
        {error && <p className="fleet-inline-error" role="alert">{error}</p>}
        <Button type="submit" variant="primary" disabled={busy}>{busy ? "Signing in…" : "Sign in"}</Button>
      </form>
    );
  }

  return (
    <>
      <div className="fleet-session-bar">
        <span><strong>{session.orgId}</strong> · {session.email} · {session.role}</span>
        <Button size="sm" variant="ghost" onClick={context.signOut}>Sign out</Button>
      </div>
      {children}
    </>
  );
}
