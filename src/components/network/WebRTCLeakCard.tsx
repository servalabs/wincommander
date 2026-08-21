import { useCallback, useState } from "react";
import { Button, Icon } from "@/components/ui/bp";
import SectionCard from "../shared/SectionCard";

interface WebRTCResult {
  leaked: boolean;
  localIPs: string[];
  error: string | null;
}

// WebRTC leak test — enumerate ICE candidates against a public STUN server.
// Any RFC1918 / link-local address present in srflx/host candidates means the
// browser is exposing the host's local network identity to remote peers.
async function runWebRTCTest(timeoutMs = 4000): Promise<WebRTCResult> {
  if (typeof RTCPeerConnection === "undefined") {
    return { leaked: false, localIPs: [], error: "WebRTC unavailable" };
  }
  const localIPs = new Set<string>();
  let pc: RTCPeerConnection | null = null;
  try {
    pc = new RTCPeerConnection({
      iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
    });
    pc.createDataChannel("");
    const offerPromise = pc.createOffer().then((o) => pc!.setLocalDescription(o));

    await new Promise<void>((resolve) => {
      const timer = setTimeout(resolve, timeoutMs);
      pc!.onicecandidate = (e) => {
        if (!e.candidate) {
          clearTimeout(timer);
          resolve();
          return;
        }
        const cand = e.candidate.candidate || "";
        const match = cand.match(/(\d{1,3}\.){3}\d{1,3}/);
        if (match) {
          const ip = match[0];
          if (
            /^10\./.test(ip) ||
            /^192\.168\./.test(ip) ||
            /^172\.(1[6-9]|2\d|3[01])\./.test(ip) ||
            /^169\.254\./.test(ip)
          ) {
            localIPs.add(ip);
          }
        }
      };
    });
    await offerPromise.catch(() => undefined);
  } catch (err) {
    return {
      leaked: false,
      localIPs: [],
      error: err instanceof Error ? err.message : String(err),
    };
  } finally {
    try { pc?.close(); } catch { /* noop */ }
  }
  return { leaked: localIPs.size > 0, localIPs: Array.from(localIPs), error: null };
}

export { runWebRTCTest };
export type { WebRTCResult };

/**
 * WebRTCHeaderButton — minimal button for use in a SectionCard headerRight slot.
 * Shows a "WebRTC" button that runs the leak test and displays a pass/fail badge.
 */
export function WebRTCHeaderButton() {
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<WebRTCResult | null>(null);

  const run = useCallback(async (e?: React.MouseEvent) => {
    e?.stopPropagation();
    setRunning(true);
    setResult(null);
    try {
      setResult(await runWebRTCTest());
    } finally {
      setRunning(false);
    }
  }, []);

  const statusColor = result
    ? result.error
      ? 'var(--color-text-muted)'
      : result.leaked
        ? 'var(--color-danger)'
        : 'var(--color-success)'
    : undefined;

  const explanation = result
    ? result.error
      ? 'WebRTC API unavailable in this environment.'
      : result.leaked
        ? `Your local IP (${result.localIPs.join(', ')}) was exposed to remote peers via WebRTC — your real network identity can leak even through a VPN.`
        : 'No local IPs exposed — WebRTC cannot leak your network identity to remote peers.'
    : undefined;

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
      {result && (
        <span
          title={explanation}
          style={{
            fontSize: 9,
            fontWeight: 800,
            letterSpacing: 0.5,
            padding: '1px 5px',
            borderRadius: 'var(--radius-sm)',
            color: statusColor,
            border: `1px solid ${statusColor}`,
            fontFamily: 'var(--font-mono)',
            cursor: 'help',
          }}
        >
          {result.error ? 'UNAVAIL' : result.leaked ? '⚠ LEAK' : '✓ SAFE'}
        </span>
      )}
      {result && explanation && (
        <span style={{
          fontSize: 9.5,
          color: statusColor,
          maxWidth: 200,
          lineHeight: 1.3,
          opacity: 0.85,
        }}>
          {explanation}
        </span>
      )}
      <Button
        small
        minimal
        icon={running ? undefined : result ? 'refresh' : 'shield'}
        loading={running}
        onClick={run}
        title={result ? 'Re-run WebRTC leak test' : 'Run WebRTC leak test'}
        text={result ? undefined : 'WebRTC'}
        style={{ fontSize: 10 }}
      />
    </div>
  );
}

/**
 * WebRTCLeakInline — compact sub-section variant. Renders as a labelled block
 * that can sit inside another SectionCard. No card chrome of its own.
 */
export function WebRTCLeakInline() {
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<WebRTCResult | null>(null);

  const run = useCallback(async () => {
    setRunning(true);
    setResult(null);
    try {
      setResult(await runWebRTCTest());
    } finally {
      setRunning(false);
    }
  }, []);

  return (
    <div
      style={{
        marginTop: 16,
        padding: "10px 12px",
        background: "var(--color-bg-secondary)",
        border: "1px solid var(--color-border)",
        borderRadius: "var(--radius-md)",
        display: "flex",
        flexDirection: "column",
        gap: 6,
      }}
    >
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8 }}>
        <span
          style={{
            fontSize: 10,
            fontWeight: 700,
            textTransform: "uppercase",
            letterSpacing: 0.6,
            color: "var(--color-text-muted)",
          }}
        >
          WebRTC Leak Test
        </span>
        <Button
          small
          minimal
          icon={running ? undefined : "refresh"}
          loading={running}
          text={running ? "Testing…" : "Run test"}
          onClick={(e: React.MouseEvent) => { e.stopPropagation(); run(); }}
        />
      </div>
      <p className="text-[var(--color-text-secondary)] font-mono text-[11px] leading-snug m-0">
        Checks whether WebRTC exposes your private LAN IP addresses to remote peers — a common VPN/identity leak.
      </p>
      {!result && !running && (
        <div className="font-mono text-[11px] text-[var(--color-text-muted)]">Not tested yet — run the test to check.</div>
      )}
      {running && (
        <div className="font-mono text-[11px] text-[var(--color-text-muted)]">Probing ICE candidates…</div>
      )}
      {result && (
        <div className="flex items-center gap-2">
          <Icon
            icon={result.error ? "info-sign" : result.leaked ? "warning-sign" : "tick-circle"}
            size={14}
            color={result.error ? "var(--color-text-muted)" : result.leaked ? "var(--color-danger)" : "var(--color-success)"}
          />
          <span
            className="font-mono text-[12px] font-bold"
            style={{ color: result.error ? "var(--color-text-muted)" : result.leaked ? "var(--color-danger)" : "var(--color-success)" }}
            title={result.leaked ? result.localIPs.join(", ") : undefined}
          >
            {result.error
              ? "WebRTC unavailable"
              : result.leaked
                ? `Leak detected — ${result.localIPs.length} local IP${result.localIPs.length === 1 ? "" : "s"} exposed`
                : "No leak detected"}
          </span>
        </div>
      )}
    </div>
  );
}

/**
 * WebRTCLeakCard — standalone variant wrapped in its own SectionCard.
 * Kept for callers that still want a dedicated card. New layouts should
 * prefer WebRTCLeakInline embedded inside another card.
 */
export default function WebRTCLeakCard() {
  return (
    <SectionCard title="WebRTC Leak Test" icon="shield">
      <WebRTCLeakInline />
    </SectionCard>
  );
}
