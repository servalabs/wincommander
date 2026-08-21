import { describe, expect, test } from "bun:test";

declare const Bun: {
    file(path: string): { text(): Promise<string> };
};

describe("Secret Settings diagnostics", () => {
    test("keeps debug actions in Dev Tools and moves runtime status above Error Center", async () => {
        const [devPanel, secretPanel] = await Promise.all([
            Bun.file("src/panels/dev/index.tsx").text(),
            Bun.file("src/panels/secret/index.tsx").text(),
        ]);

        expect(devPanel).not.toContain("Runtime Status");
        expect(devPanel).not.toContain("test_pro_handshake");
        expect(devPanel).not.toContain("fleet_status");
        expect(devPanel).toContain("Test Actions");
        expect(secretPanel.indexOf("<RuntimeStatusSection />")).toBeGreaterThan(-1);
        expect(secretPanel.indexOf("<RuntimeStatusSection />") < secretPanel.indexOf('title="Error Center"')).toBe(true);
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

    test("lets Error Center consume remaining height and respond to its container", async () => {
        const [secretStyles, logStyles] = await Promise.all([
            Bun.file("src/panels/secret/index.css").text(),
            Bun.file("src/panels/privacy/LogViewer.css").text(),
        ]);

        expect(secretStyles).toContain("grid-template-rows: auto minmax(260px, 1fr)");
        expect(secretStyles).toContain(".secret-diagnostics-log-card > .section-collapse");
        expect(logStyles).toContain("flex: 1 1 260px");
        expect(logStyles).toContain("min-height: 0");
        expect(logStyles).toContain("@container (max-width: 760px)");
        expect(logStyles).not.toContain("max-height: 340px");
    });
});
