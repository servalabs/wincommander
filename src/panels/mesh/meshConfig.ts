import type { MeshVPNStatus } from "../../hooks/useBackend";

export interface MeshConfigDraft {
    advertiseExitNode: boolean;
    allowLanAccess: boolean;
    unattended: boolean;
    acceptRoutes: boolean;
    acceptDNS: boolean;
    shieldsUp: boolean;
    exitNodeIP: string;
}

export interface MeshConfigPayload {
    AdvertiseExitNode: boolean;
    AllowLanAccess: boolean;
    Unattended: boolean;
    AcceptRoutes: boolean;
    AcceptDNS: boolean;
    ExitNodeIP: string;
    ShieldsUp: boolean;
}

export function meshDraftFromPrefs(prefs?: MeshVPNStatus["prefs"] | null): MeshConfigDraft {
    return {
        advertiseExitNode: !!prefs?.AdvertiseExitNode,
        allowLanAccess: !!prefs?.ExitNodeAllowLANAccess,
        unattended: !!prefs?.Unattended,
        acceptRoutes: !!prefs?.AcceptRoutes,
        acceptDNS: !!prefs?.AcceptDNS,
        shieldsUp: !!prefs?.ShieldsUp,
        exitNodeIP: prefs?.ExitNodeIP || "",
    };
}

export function meshConfigPayload(draft: MeshConfigDraft): MeshConfigPayload {
    return {
        AdvertiseExitNode: draft.advertiseExitNode,
        AllowLanAccess: draft.allowLanAccess,
        Unattended: draft.unattended,
        AcceptRoutes: draft.acceptRoutes,
        AcceptDNS: draft.acceptDNS,
        ExitNodeIP: draft.exitNodeIP,
        ShieldsUp: draft.shieldsUp,
    };
}

export function meshPrefsMatchConfig(
    prefs: MeshVPNStatus["prefs"] | null | undefined,
    config: MeshConfigPayload,
): boolean {
    return meshPrefsMismatches(prefs, config).length === 0;
}

export function meshPrefsMismatches(
    prefs: MeshVPNStatus["prefs"] | null | undefined,
    config: MeshConfigPayload,
): string[] {
    if (!prefs) return ["settings unavailable"];

    const mismatches: string[] = [];
    if (!!prefs.AdvertiseExitNode !== config.AdvertiseExitNode) mismatches.push("Exit node advertisement");
    if (!!prefs.ExitNodeAllowLANAccess !== config.AllowLanAccess) mismatches.push("LAN access through exit node");
    if (!!prefs.Unattended !== config.Unattended) mismatches.push("Run unattended");
    if (!!prefs.AcceptRoutes !== config.AcceptRoutes) mismatches.push("Accept subnet routes");
    if (!!prefs.AcceptDNS !== config.AcceptDNS) mismatches.push("Use Tailscale DNS");
    if (!!prefs.ShieldsUp !== config.ShieldsUp) mismatches.push("Block incoming connections");
    if ((prefs.ExitNodeIP || "").trim().toLowerCase() !== config.ExitNodeIP.trim().toLowerCase()) {
        mismatches.push("Exit node selection");
    }
    return mismatches;
}

function meshConfigsEqual(left: MeshConfigPayload, right: MeshConfigPayload): boolean {
    return (
        left.AdvertiseExitNode === right.AdvertiseExitNode &&
        left.AllowLanAccess === right.AllowLanAccess &&
        left.Unattended === right.Unattended &&
        left.AcceptRoutes === right.AcceptRoutes &&
        left.AcceptDNS === right.AcceptDNS &&
        left.ShieldsUp === right.ShieldsUp &&
        left.ExitNodeIP.trim().toLowerCase() === right.ExitNodeIP.trim().toLowerCase()
    );
}

export function shouldSyncMeshDraftFromStatus(args: {
    prefs?: MeshVPNStatus["prefs"] | null;
    hasLocalChanges: boolean;
    pendingApply: MeshConfigPayload | null;
    currentDraft: MeshConfigDraft;
}): { syncDraft: boolean; clearPendingApply: boolean } {
    if (!args.pendingApply) {
        return { syncDraft: !!args.prefs && !args.hasLocalChanges, clearPendingApply: false };
    }

    if (!meshPrefsMatchConfig(args.prefs, args.pendingApply)) {
        return { syncDraft: false, clearPendingApply: false };
    }

    const formStillMatchesAppliedConfig = meshConfigsEqual(
        meshConfigPayload(args.currentDraft),
        args.pendingApply,
    );
    return { syncDraft: formStillMatchesAppliedConfig, clearPendingApply: true };
}
