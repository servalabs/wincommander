import { describe, expect, test } from "bun:test";

declare const Bun: {
    file(path: string): { text(): Promise<string> };
};

describe("Secret Settings Auto Start", () => {
    test("reads the native task state and commits a toggle only after Windows confirms it", async () => {
        const source = await Bun.file("src/panels/secret/index.tsx").text();

        expect(source).toContain('invoke<boolean>("is_autostart_enabled")');
        expect(source).toContain('await invoke(next ? "enable_autostart_task" : "remove_autostart_task")');
        expect(source).toContain("if (confirmed !== next)");
        expect(source).toContain("setAutostartEnabled(confirmed)");
        expect(source).not.toContain("setAutostartEnabled(next)");
    });

    test("keeps the control unavailable while its state is unknown and explains both outcomes", async () => {
        const source = await Bun.file("src/panels/secret/index.tsx").text();

        expect(source).toContain("Checking Windows startup…");
        expect(source).toContain("all WinCommander startup entries");
        expect(source).toContain("supported Windows sign-in task");
        expect(source).toContain('role="status"');
        expect(source).toContain('role="alert"');
        expect(source).toContain("setAutostartEnabled(null)");
        expect(source).toContain('setAutostartEnabled(await invoke<boolean>("is_autostart_enabled"))');
        expect(source).not.toContain("Auto Start wasn't changed");
    });

    test("retries a false status only while the panel first hydrates", async () => {
        const source = await Bun.file("src/panels/secret/index.tsx").text();

        expect(source).toContain("INITIAL_AUTOSTART_STATUS_RETRIES = 3");
        expect(source).toContain("readAutostartStatus(true)");
        expect(source).toContain("readAutostartStatus(false)");
        expect(source).toContain("retryInitialFalse && !enabled");
    });

    test("lets Windows request elevation instead of requiring an already-elevated app", async () => {
        const source = await Bun.file("src/panels/secret/index.tsx").text();
        const autoStartTile = source.slice(
            source.indexOf("function AutoStartTile()"),
            source.indexOf("function LockDisguiseSection()"),
        );

        expect(autoStartTile).toContain("disabled={autostartPolicyLocked}");
        expect(autoStartTile).not.toContain("needsElevation");
        expect(autoStartTile).toContain("Windows will ask for approval if needed.");
    });
});
