import { describe, expect, test } from "bun:test";

declare const Bun: {
    file(path: string): { text(): Promise<string> };
};

describe("Secret Settings diagnostics", () => {
    test("keeps debug actions in Dev Tools and moves runtime status above Diagnostic Center", async () => {
        const [devPanel, secretPanel] = await Promise.all([
            Bun.file("src/panels/dev/index.tsx").text(),
            Bun.file("src/panels/secret/index.tsx").text(),
        ]);

        expect(devPanel).not.toContain("Runtime Status");
        expect(devPanel).not.toContain("test_pro_handshake");
        expect(devPanel).not.toContain("fleet_status");
        expect(devPanel).toContain("Test Actions");
        expect(secretPanel.indexOf("<RuntimeStatusSection />")).toBeGreaterThan(-1);
        expect(secretPanel.indexOf("<RuntimeStatusSection />") < secretPanel.indexOf('title="Diagnostic Center"')).toBe(true);
    });

    test("uses the Pro handshake ok field and never starts that smoke test on mount", async () => {
        const [hook, section] = await Promise.all([
            Bun.file("src/hooks/useRuntimeDiagnostics.ts").text(),
            Bun.file("src/panels/secret/RuntimeStatusSection.tsx").text(),
        ]);

        expect(hook).toContain('invoke<ProHandshakeResult>("test_pro_handshake")');
        expect(hook).toContain("ok: boolean");
        expect(section).toContain("proStatus.ok");
        expect(hook).not.toContain("void testProConnection()");
    });

    test("keeps one Diagnostic Center below runtime status without the duplicate Error Center", async () => {
        const [secretStyles, secretPanel] = await Promise.all([
            Bun.file("src/panels/secret/index.css").text(),
            Bun.file("src/panels/secret/index.tsx").text(),
        ]);

        expect(secretStyles).toContain("grid-template-rows: auto minmax(260px, 1fr)");
        expect(secretStyles).toContain(".secret-diagnostics-log-card > .section-collapse");
        expect(secretPanel).toContain('title="Diagnostic Center"');
        expect(secretPanel).not.toContain('title="Error Center"');
        expect(secretPanel).not.toContain("LogViewer");
    });

    test("offers the developer context-menu switch only after the native debug-build check", async () => {
        const [secretPanel, app, gate] = await Promise.all([
            Bun.file("src/panels/secret/index.tsx").text(),
            Bun.file("src/App.tsx").text(),
            Bun.file("src/lib/developerContextMenu.ts").text(),
        ]);

        expect(secretPanel).toContain('invoke<boolean>("is_dev_build")');
        expect(secretPanel).toContain("if (isNativeDebugBuild !== true) return null");
        expect(secretPanel).toContain("developerContextMenuEnabled");
        expect(app).toContain("shouldAllowDeveloperContextMenu");
        expect(app).toContain('window.addEventListener("contextmenu", suppressBrowserContextMenu, true)');
        expect(gate).toContain("isNativeDebugBuild && userOptedIn === true");
    });

    test("keeps the developer console as an in-app, debug-only, bounded diagnostic feed", async () => {
        const [secretPanel, flowLogger, logStore] = await Promise.all([
            Bun.file("src/panels/secret/index.tsx").text(),
            Bun.file("src/components/FlowActivityLogger.tsx").text(),
            Bun.file("src/lib/developerDiagnosticsLog.ts").text(),
        ]);

        expect(secretPanel).toContain("Developer console log");
        expect(secretPanel).toContain("developerDiagnosticsLogEnabled");
        expect(secretPanel).toContain("if (isNativeDebugBuild !== true) return null");
        expect(flowLogger).toContain("recordDeveloperFlowDiagnostic(message)");
        expect(logStore).toContain("MAX_DEVELOPER_DIAGNOSTIC_ENTRIES = 80");
        expect(logStore).toContain("isNativeDebugBuild && userOptedIn === true");
        expect(logStore).toContain("entries = []");
        expect(logStore).not.toContain("console.");
    });
});
