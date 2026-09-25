import { describe, expect, test } from "bun:test";
import {
    meshConfigPayload,
    meshDraftFromPrefs,
    meshPrefsMatchConfig,
    shouldSyncMeshDraftFromStatus,
} from "./meshConfig";

const draft = {
    advertiseExitNode: true,
    allowLanAccess: false,
    unattended: true,
    acceptRoutes: false,
    acceptDNS: true,
    shieldsUp: true,
    exitNodeIP: "100.64.0.20",
};

const prefs = {
    AdvertiseExitNode: true,
    ExitNodeIP: "100.64.0.20",
    ExitNodeAllowLANAccess: false,
    ShieldsUp: true,
    Unattended: true,
    AcceptRoutes: false,
    AcceptDNS: true,
};

describe("private network config apply state", () => {
    test("converts the latest edited draft into the complete apply payload", () => {
        expect(meshConfigPayload(draft)).toEqual({
            AdvertiseExitNode: true,
            AllowLanAccess: false,
            Unattended: true,
            AcceptRoutes: false,
            AcceptDNS: true,
            ExitNodeIP: "100.64.0.20",
            ShieldsUp: true,
        });
    });

    test("confirms all requested values against the live preferences", () => {
        expect(meshPrefsMatchConfig(prefs, meshConfigPayload(draft))).toBe(true);
        expect(meshPrefsMatchConfig({ ...prefs, AcceptDNS: false }, meshConfigPayload(draft))).toBe(false);
        expect(meshPrefsMatchConfig(undefined, meshConfigPayload(draft))).toBe(false);
    });

    test("does not replace an applied draft with stale status before it matches", () => {
        const stale = shouldSyncMeshDraftFromStatus({
            prefs: { ...prefs, AcceptDNS: false },
            hasLocalChanges: true,
            pendingApply: meshConfigPayload(draft),
            currentDraft: draft,
        });
        expect(stale).toEqual({ syncDraft: false, clearPendingApply: false });
    });

    test("syncs the submitted values after readback confirms them", () => {
        expect(shouldSyncMeshDraftFromStatus({
            prefs,
            hasLocalChanges: true,
            pendingApply: meshConfigPayload(draft),
            currentDraft: draft,
        })).toEqual({ syncDraft: true, clearPendingApply: true });
    });

    test("clears a confirmed apply without overwriting edits made while it was running", () => {
        const newerDraft = { ...draft, shieldsUp: false };
        const confirmed = shouldSyncMeshDraftFromStatus({
            prefs,
            hasLocalChanges: true,
            pendingApply: meshConfigPayload(draft),
            currentDraft: newerDraft,
        });
        expect(confirmed).toEqual({ syncDraft: false, clearPendingApply: true });
    });

    test("seeds a local draft from refreshed status when it has no pending edits", () => {
        expect(meshDraftFromPrefs(prefs)).toEqual(draft);
        expect(shouldSyncMeshDraftFromStatus({
            prefs,
            hasLocalChanges: false,
            pendingApply: null,
            currentDraft: draft,
        })).toEqual({ syncDraft: true, clearPendingApply: false });
    });

    test("manual refresh keeps an unsubmitted local edit", () => {
        expect(shouldSyncMeshDraftFromStatus({
            prefs,
            hasLocalChanges: true,
            pendingApply: null,
            currentDraft: { ...draft, acceptDNS: false },
        })).toEqual({ syncDraft: false, clearPendingApply: false });
    });
});
