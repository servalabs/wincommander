import { expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

test("search-result shredding requires the shared target-bound native capability", async () => {
  const [hook, actionBackend, shredderFacade, shredderCore, app] = await Promise.all([
    Bun.file("src/hooks/useSearchResultContextMenu.ts").text(),
    Bun.file("src-tauri/commander-free/src/search_actions.rs").text(),
    Bun.file("src-tauri/commander-free/src/context_menu_shred.rs").text(),
    Bun.file("src-tauri/commander-context-shred/src/lib.rs").text(),
    Bun.file("src-tauri/commander-free/src/lib.rs").text(),
  ]);

  expect(hook).toContain('requestDestructiveCapability({ command: "secure_erase", path: target.path })');
  expect(hook).toContain('invoke("search_shred_direct", { path: target.path, capabilityToken })');
  expect(hook).not.toContain('emit("shred-requested"');
  expect(actionBackend).toContain("pub async fn search_shred_direct");
  expect(actionBackend).toContain("crate::context_menu_shred::secure_shred(path, capability_token).await");
  expect(shredderFacade).toContain("commander_context_shred::execute_cli(raw_paths)");
  expect(shredderCore).toContain("validate_selection(&raw_paths)?");
  expect(shredderCore).toContain("validate_target(&initially_validated_target.to_string_lossy())?");
  expect(app).toContain("search_actions::search_shred_direct");
});
