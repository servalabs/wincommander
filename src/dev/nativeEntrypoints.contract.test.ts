import { describe, expect, test } from "bun:test";

const read = async (path: string) =>
  (await fetch(new URL(`../../${path}`, import.meta.url))).text();

describe("native and UI-audit entrypoints", () => {
  test("normal Tauri development uses the real frontend without audit mocks", async () => {
    const [rootHtml, auditHtml, cliHtml, tauriConfig, nativeSource, cliSource] = await Promise.all([
      read("index.html"),
      read("ui-audit.html"),
      read("public/cli-runtime.html"),
      read("src-tauri/commander-free/tauri.conf.json"),
      read("src-tauri/commander-free/src/lib.rs"),
      read("src-tauri/commander-free/src/cli.rs"),
    ]);

    expect(rootHtml).toContain('src="/src/main.tsx"');
    expect(rootHtml).not.toContain("uiAuditBootstrap");
    expect(auditHtml).toContain('src="/src/dev/uiAuditBootstrap.ts"');
    expect(cliHtml).not.toContain("/src/main.tsx");
    expect(cliHtml).not.toContain("uiAuditBootstrap");

    const config = JSON.parse(tauriConfig) as { build: { devUrl: string } };
    expect(config.build.devUrl).toBe("http://127.0.0.1:1420");
    expect(nativeSource).toMatch(
      /"search-overlay",\s*tauri::WebviewUrl::App\("index\.html"\.into\(\)\)/s,
    );
    expect(nativeSource).toMatch(
      /"main",\s*tauri::WebviewUrl::App\("cli-runtime\.html"\.into\(\)\)/s,
    );
    expect(cliSource).toMatch(
      /fn run_backend[\s\S]*?context\.config_mut\(\)\.app\.windows\.clear\(\);[\s\S]*?builder\.run\(context\)/,
    );
  });

  test("plain browser root refuses to impersonate a connected desktop app", async () => {
    const source = await read("src/main.tsx");

    expect(source).toContain("const hasNativeBackend = Boolean");
    expect(source).toContain("Native backend disconnected");
    expect(source).toContain("bun run dev:tauri:free");
    expect(source).toContain("if (!hasNativeBackend)");
  });
});
