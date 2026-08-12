import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): {
    json<T>(): Promise<T>;
  };
};

type TauriConfig = {
  app?: {
    security?: {
      csp?: string;
    };
  };
};

function parseDirectives(csp: string): Record<string, string[]> {
  return Object.fromEntries(
    csp
      .split(";")
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) => {
        const [name, ...values] = part.split(/\s+/);
        return [name, values];
      }),
  );
}

describe("Tauri CSP", () => {
  test("connect-src uses explicit endpoints instead of scheme wildcards", async () => {
    const config = await Bun.file("src-tauri/commander-free/tauri.conf.json").json<TauriConfig>();
    const directives = parseDirectives(config.app?.security?.csp ?? "");
    const connectSrc = directives["connect-src"] ?? [];

    expect(connectSrc).toContain("'self'");
    expect(connectSrc).toContain("tauri:");
    expect(connectSrc).toContain("ipc:");
    expect(connectSrc).toContain("http://ipc.localhost");
    expect(connectSrc).toContain("http://tauri.localhost");
    expect(connectSrc).toContain("https://winupdates.servalabs.com");
    expect(connectSrc).toContain("https://wincommander-licensing.servalabs.com");
    expect(connectSrc).not.toContain("http:");
    expect(connectSrc).not.toContain("https:");
    expect(connectSrc).not.toContain("wss:");
  });

  test("script-src does not allow inline scripts or eval", async () => {
    const config = await Bun.file("src-tauri/commander-free/tauri.conf.json").json<TauriConfig>();
    const directives = parseDirectives(config.app?.security?.csp ?? "");
    const scriptSrc = directives["script-src"] ?? [];

    expect(scriptSrc).not.toContain("'unsafe-inline'");
    expect(scriptSrc).not.toContain("'unsafe-eval'");
  });
});
