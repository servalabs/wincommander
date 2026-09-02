import { describe, expect, test } from "bun:test";
import { isSignedInAccountCovered } from "./DiskCleanupGranular";

describe("disk cleanup schedule ownership", () => {
    const signedInAccount = {
        name: "alex",
        displayName: "Alex Smith",
        sid: "S-1-5-21-100-200-300-400",
    };

    test("covers only a task whose Scheduler principal matches the signed-in account", () => {
        expect(isSignedInAccountCovered(
            { taskName: "WinCommander_AutoErase_diskCleanup", ownerAccount: "S-1-5-21-100-200-300-400" },
            signedInAccount,
        )).toBe(true);
    });

    test("does not treat an unsuffixed canonical task name as signed-in coverage", () => {
        expect(isSignedInAccountCovered(
            { taskName: "WinCommander_AutoErase_diskCleanup", ownerAccount: "DOMAIN\\other-user" },
            signedInAccount,
        )).toBe(false);
    });

    test("keeps domain-qualified owners distinct from an unqualified profile label", () => {
        expect(isSignedInAccountCovered(
            { taskName: "WinCommander_AutoErase_diskCleanup", ownerAccount: "DOMAIN\\alex" },
            { name: "alex", displayName: "Alex Smith" },
        )).toBe(false);
    });
});
