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
    if (!prefs) return false;
    return (
        !!prefs.AdvertiseExitNode === config.AdvertiseExitNode &&
        !!prefs.ExitNodeAllowLANAccess === config.AllowLanAccess &&
        !!prefs.Unattended === config.Unattended &&
        !!prefs.AcceptRoutes === config.AcceptRoutes &&
        !!prefs.AcceptDNS === config.AcceptDNS &&
        !!prefs.ShieldsUp === config.ShieldsUp &&
        (prefs.ExitNodeIP || "").trim().toLowerCase() === config.ExitNodeIP.trim().toLowerCase()
    );
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
