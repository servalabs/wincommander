import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const root = new URL("../../src-tauri/commander-free/", import.meta.url);
const read = (path: string) => readFileSync(new URL(path, root), "utf8");

describe("embedded view privilege boundary", () => {
  test("parent main window and external child labels cannot inherit desktop capabilities", () => {
    const capability = JSON.parse(read("capabilities/default.json"));
    expect(capability.windows).toBeUndefined();
    expect(capability.webviews).toEqual(["main", "search-overlay"]);
    expect(capability.remote).toBeUndefined();
  });

  test("the alert renderer cannot emit arbitrary application events", () => {
    const capability = JSON.parse(read("capabilities/notification-alerts.json"));
    expect(capability.windows).toBeUndefined();
    for (const permission of ["core:default", "core:event:default", "core:event:allow-emit", "core:event:allow-emit-to"]) {
      expect(capability.permissions).not.toContain(permission);
    }
    expect(read("src/native_notify.rs")).toContain('app.emit("wc-custom-notification-ready", ())');
  });

  test("all custom commands pass the native webview guard", () => {
    const source = read("src/lib.rs");
    expect(source.match(/\.invoke_handler\(/g)).toHaveLength(1);
    expect(source).toContain(".invoke_handler(ipc_boundary::guard(tauri::generate_handler![");
  });

  test("embedded-view input checks precede storage deletion and webview changes", () => {
    const source = read("src/server_apps.rs");
    const checked = source.indexOf("crate::server_app_policy::validate_url(&parsed_url");
    expect(checked).toBeGreaterThan(-1);
    expect(checked).toBeLessThan(source.indexOf("std::fs::remove_dir_all(&p)"));
    expect(checked).toBeLessThan(source.indexOf("wv.hide()"));
    expect(source).toContain(".on_navigation(move |url|");
  });
});
