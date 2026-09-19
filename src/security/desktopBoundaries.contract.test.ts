import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "../..");
const read = (path: string) => readFileSync(join(root, path), "utf8");

describe("desktop trust boundaries", () => {
  test("renderer has no direct filesystem or process-pipe permissions", () => {
    const capability = JSON.parse(read("src-tauri/commander-free/capabilities/default.json"));
    const permissions = capability.permissions.map((p: string | { identifier: string }) =>
      typeof p === "string" ? p : p.identifier);
    expect(permissions.filter((p: string) => p.startsWith("fs:"))).toEqual([]);
    expect(permissions).not.toContain("shell:allow-kill");
    expect(permissions).not.toContain("shell:allow-stdin-write");
    expect(permissions).not.toContain("updater:default");
  });

  test("file-transfer callers cannot choose a path or arbitrary exported bytes", () => {
    const hooks = read("src/hooks/useSettings.ts");
    expect(hooks).not.toMatch(/invoke[^\n]*write_settings_export_file[^\n]*\{\s*path/);
    expect(hooks).not.toMatch(/invoke[^\n]*read_settings_import_file[^\n]*\{\s*path/);
    expect(read("src-tauri/commander-free/src/settings.rs"))
      .not.toContain("pub fn write_settings_export_file(path: String, content: String)");
  });

  test("the default capability cannot be inherited by child webviews", () => {
    const capability = JSON.parse(read("src-tauri/commander-free/capabilities/default.json"));
    expect(capability.windows).toBeUndefined();
    expect(capability.webviews).toEqual(["main", "search-overlay"]);
    expect(read("src-tauri/commander-free/src/lib.rs"))
      .toContain(".invoke_handler(ipc_boundary::guard(tauri::generate_handler![");
  });

  test("elevated repair does not trust inherited directory or command lookup", () => {
    const source = read("src-tauri/commander-free/src/service_repair.rs");
    expect(source).not.toContain('var_os("ProgramW6432")');
    expect(source).not.toContain('var_os("ProgramFiles")');
    expect(source).not.toContain('Command::new("sc.exe")');
    expect(source).toContain("service_repair_paths::protected_install_dir()");
    expect(source).toContain("service_repair_paths::service_control_executable()");
  });

  test("release CSP does not expose development servers", () => {
    const { app: { security } } = JSON.parse(read("src-tauri/commander-free/tauri.conf.json"));
    expect(security.csp).not.toMatch(/(?:localhost|127\.0\.0\.1|\[::1\]):(?:1420|8787)/);
    expect(security.devCsp).toContain("http://127.0.0.1:1420");
    expect(security.csp).toContain("object-src 'none'");
    expect(security.csp).toContain("form-action 'none'");
  });
});
