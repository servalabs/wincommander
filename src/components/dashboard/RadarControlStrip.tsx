import { useCallback, useEffect, useRef, useState } from "react";
import { Globe, Server } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useAppState } from "../../context/AppContext";
import type { DNSStatus } from "../../hooks/useBackend";

const IP_REFRESH_MS = 15_000;
/** How long a cached public IP is considered fresh before we re-probe. */
const IP_TTL_MS = 300_000; // 5 min

// Module-level cache so revisiting the dashboard shows the last known IP
// immediately instead of blanking to "checking…" and re-probing every mount.
let cachedIp: string | null = null;
let cachedIpAt = 0;

/** Friendly DNS label, mirroring the mapping used in PublicIPCard. */
function dnsLabel(s: DNSStatus | null): string {
  if (!s) return "checking…";
  if (s.provider === "AdGuard_Ads_Trackers") return "Ads + Trackers";
  if (s.provider === "Cloudflare_Malware_Adult") return "Malware + Adult";
  if (s.provider === "Swiss_Firewall") return "ServaLabs Netwall";
  if (s.provider === "ControlD") return "Simple Firewall";
  const ip = s.resolverIp ?? s.servers?.[0] ?? null;
  const org = s.resolverOrg ?? null;
  if (!ip) return "—";
  return org ? `${org}` : ip;
}

/**
 * RadarControlStrip — a horizontal strip directly under the radar showing the
 * live Public IP + DNS readouts (owner request: these live near the radar only;
 * the Camera/Mic/Internet toggles moved to the left column PrivacyTogglesCard).
 */
export default function RadarControlStrip() {
  const { networkDnsStatus } = useAppState();
  // Seed from the module-level cache so a revisit renders the last IP instantly.
  const [ip, setIp] = useState<string | null>(cachedIp);
  const inFlight = useRef(false);

  const refreshIp = useCallback(async () => {
    if (inFlight.current) return;
    // Staleness guard: skip the probe entirely if the cached value is still fresh.
    if (cachedIp !== null && Date.now() - cachedIpAt < IP_TTL_MS) {
      setIp(cachedIp);
      return;
    }
    inFlight.current = true;
    try {
      const trace = await invoke<{ ip: string | null }>("get_public_ip_trace");
      cachedIp = trace.ip ?? null;
      cachedIpAt = Date.now();
      setIp(cachedIp);
    } catch {
      // best-effort — leave the previous value on probe failure
    } finally {
      inFlight.current = false;
    }
  }, []);

  useEffect(() => {
    refreshIp();
    // Keep the 15s cadence, but refreshIp self-guards on the 5-min TTL so the
    // probe only actually fires when the cached value has gone stale.
    const id = setInterval(refreshIp, IP_REFRESH_MS);
    return () => clearInterval(id);
  }, [refreshIp]);

  return (
    <div className="radar-control-strip">
      <div className="radar-strip-readout" title={ip ?? undefined}>
        <Globe size={12} className="rsr-icon" />
        <span className="rsr-label">PUBLIC IP</span>
        <span className="rsr-value">{ip ?? "checking…"}</span>
      </div>

      <div className="radar-strip-readout" title={networkDnsStatus?.resolverIp ?? networkDnsStatus?.servers?.join(", ") ?? undefined}>
        <Server size={12} className="rsr-icon" />
        <span className="rsr-label">DNS</span>
        <span className="rsr-value">{dnsLabel(networkDnsStatus)}</span>
      </div>
    </div>
  );
}
