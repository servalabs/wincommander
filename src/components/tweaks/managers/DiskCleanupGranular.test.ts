import { describe, expect, test } from "bun:test";
import { diskCleanupErrorMessage, isSignedInAccountCovered } from "./DiskCleanupGranular";

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

describe("disk cleanup permission copy", () => {
    test("replaces raw administrator diagnostics with a concise next step", () => {
        expect(diskCleanupErrorMessage("Administrator privileges required. Command: Get-DiskCleanupScan At line:10 char:9"))
            .toBe("Cleaning Windows-managed storage requires administrator permission. Run WinCommander as administrator to continue.");
    });

    test("keeps an unrelated cleanup error useful", () => {
        expect(diskCleanupErrorMessage("The selected drive is unavailable."))
            .toBe("The selected drive is unavailable.");
    });
});
