import { describe, expect, test } from "bun:test";
import { isMeshPeerOnline, isMeshPeerStale, parseMeshLastSeen } from "./meshPresence";

describe("mesh peer presence", () => {
    test("uses only a literal true Online value for live presence", () => {
        expect(isMeshPeerOnline({ Online: true })).toBe(true);
        // Runtime IPC data is untrusted: these must never become truthy peers.
        expect(isMeshPeerOnline({ Online: "false" as unknown as boolean })).toBe(false);
        expect(isMeshPeerOnline({ Online: 1 as unknown as boolean })).toBe(false);
        expect(isMeshPeerOnline({ Online: false })).toBe(false);
    });

    test("does not confuse recent traffic with online presence", () => {
        expect(isMeshPeerOnline({ Online: false })).toBe(false);
        // `Active` is intentionally irrelevant: it is recent traffic, not
        // Tailscale's connected-to-control-plane liveness bit.
        expect(isMeshPeerOnline({ Online: true })).toBe(true);
    });

    test("uses LastSeen only to classify already-offline peers as stale", () => {
        const now = Date.parse("2026-08-20T12:00:00Z");
        const oneHour = 60 * 60 * 1000;
        expect(isMeshPeerStale({ Online: false, LastSeen: "2026-08-20T10:00:00Z" }, oneHour, now)).toBe(true);
        expect(isMeshPeerStale({ Online: false, LastSeen: "2026-08-20T11:30:00Z" }, oneHour, now)).toBe(false);
        expect(isMeshPeerStale({ Online: true, LastSeen: "2026-08-01T00:00:00Z" }, oneHour, now)).toBe(false);
    });

    test("fails safe for missing, malformed, and future LastSeen values", () => {
        const now = Date.parse("2026-08-20T12:00:00Z");
        const threshold = 60 * 60 * 1000;
        expect(parseMeshLastSeen("not-a-date")).toBeNull();
        expect(isMeshPeerStale({ Online: false, LastSeen: undefined }, threshold, now)).toBe(false);
        expect(isMeshPeerStale({ Online: false, LastSeen: "not-a-date" }, threshold, now)).toBe(false);
        expect(isMeshPeerStale({ Online: false, LastSeen: "2026-08-21T12:00:00Z" }, threshold, now)).toBe(false);
    });
});
