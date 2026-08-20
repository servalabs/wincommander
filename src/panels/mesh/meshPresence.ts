import type { MeshVPNPeer } from "../../hooks/useBackend";

/**
 * Tailscale's `Online` bit is the only peer liveness signal. `Active` is
 * deliberately not used here: it only means this computer exchanged traffic
 * with the peer recently, so an idle but reachable device is still online.
 *
 * This strict comparison also fails closed when an older/broken backend sends
 * a string such as "false" instead of a JSON boolean. JavaScript considers
 * that non-empty string truthy, which previously made an offline peer appear
 * active.
 */
export function isMeshPeerOnline(peer: Pick<MeshVPNPeer, "Online">): boolean {
    return peer.Online === true;
}

/** Returns a valid LastSeen instant, or null for missing/malformed data. */
export function parseMeshLastSeen(lastSeen: string | null | undefined): number | null {
    if (!lastSeen) return null;
    const timestamp = new Date(lastSeen).getTime();
    return Number.isFinite(timestamp) ? timestamp : null;
}

/**
 * LastSeen is only meaningful for an offline Tailscale peer. Never let a
 * stale/malformed timestamp override a live `Online: true` status.
 */
export function isMeshPeerStale(
    peer: Pick<MeshVPNPeer, "Online" | "LastSeen">,
    staleThresholdMs: number,
    nowMs = Date.now(),
): boolean {
    if (isMeshPeerOnline(peer)) return false;
    const lastSeenMs = parseMeshLastSeen(peer.LastSeen);
    return lastSeenMs !== null && nowMs >= lastSeenMs && nowMs - lastSeenMs >= staleThresholdMs;
}
