import { expect, test } from "bun:test";
const read = (path: string) => Bun.file(path).text();

test("arbitrary policy epochs are not a renderer command", async () => {
  expect(await read("src-tauri/commander-free/src/lib.rs")).not.toContain("settings::apply_admin_config_cmd,");
  expect(await read("src/hooks/useSettings.ts")).not.toContain("'apply_admin_config_cmd'");
});

test("all local settings replacement paths share the native policy gate", async () => {
  const settings = await read("src-tauri/commander-free/src/settings.rs");
  expect(settings).toContain("local_write::Mutation::Patch(patch)");
  expect(settings).toContain("local_write::Mutation::Replace(settings)");
  expect(settings).toContain("local_write::Mutation::Import(json.to_string())");
});
