import { expect, test } from "bun:test";

test("ActivityWatch native HTTP never uses ambient proxies or follows redirects", async () => {
  const source = await Bun.file("src-tauri/commander-free/src/activity_watch_autostart.rs").text();
  const delegated = await Bun.file("src-tauri/commander-free/src/activity_watch_http.rs").exists();
  const transport = delegated ? await Bun.file("src-tauri/commander-free/src/activity_watch_http.rs").text() : source;
  expect(transport).toContain(".no_proxy()");
  expect(transport).toContain("reqwest::redirect::Policy::none()");
  expect(transport).toMatch(/\.chunk\(\)\s*\.await/);
});
