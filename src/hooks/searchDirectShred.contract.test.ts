import { expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

test("search-result shredding uses the guarded direct backend command, not the modal event", async () => {
  const [hook, actionBackend, shredder, app] = await Promise.all([
    Bun.file("src/hooks/useSearchResultContextMenu.ts").text(),
    Bun.file("src-tauri/commander-free/src/search_actions.rs").text(),
    Bun.file("src-tauri/commander-free/src/context_menu_shred.rs").text(),
    Bun.file("src-tauri/commander-free/src/lib.rs").text(),
  ]);

  expect(hook).toContain('invoke("search_shred_direct", { path: target.path })');
  expect(hook).not.toContain('emit("shred-requested"');
  expect(actionBackend).toContain("pub async fn search_shred_direct");
  expect(actionBackend).toContain("crate::context_menu_shred::execute(app, vec![path]).await");
  expect(shredder).toContain("validate_target(&raw_paths[0])?");
  expect(app).toContain("search_actions::search_shred_direct");
});
