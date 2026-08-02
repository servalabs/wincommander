import { describe, expect, test } from "bun:test";

const read = async (path: string) =>
  (await fetch(new URL(`../../${path}`, import.meta.url))).text();

describe("native and UI-audit entrypoints", () => {
  test("normal Tauri development uses the real frontend without audit mocks", async () => {
    const [rootHtml, auditHtml, tauriConfig] = await Promise.all([
      read("index.html"),
      read("ui-audit.html"),
      read("src-tauri/commander-free/tauri.conf.json"),
    ]);

    expect(rootHtml).toContain('src="/src/main.tsx"');
    expect(rootHtml).not.toContain("uiAuditBootstrap");
    expect(auditHtml).toContain('src="/src/dev/uiAuditBootstrap.ts"');

    const config = JSON.parse(tauriConfig) as { build: { devUrl: string } };
    expect(config.build.devUrl).toBe("http://localhost:1420");
  });

  test("plain browser root refuses to impersonate a connected desktop app", async () => {
    const source = await read("src/main.tsx");

    expect(source).toContain("const hasNativeBackend = Boolean");
    expect(source).toContain("Native backend disconnected");
    expect(source).toContain("bun run dev:tauri:free");
    expect(source).toContain("if (!hasNativeBackend)");
  });
});
