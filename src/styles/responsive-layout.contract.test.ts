import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const read = (path: string) => readFileSync(path, "utf8");

test("panel layouts size from the available app stage", () => {
  const shell = read("src/components/AppShell.tsx");
  const globalStyles = read("src/index.css");
  const appsStyles = read("src/panels/apps/index.css");
  const vaultStyles = read("src/panels/vault/index.css");
  const cleanupStyles = read("src/components/cleanup/CleanupTraceCard.css");

  expect(shell).toContain("app-panel-stage");
  expect(globalStyles).toContain(".app-panel-stage .panel-container");
  expect(globalStyles).toContain("max-width: none;");
  expect(appsStyles).toContain(".apps-panel");
  expect(appsStyles).not.toContain(".panel-container {");
  expect(vaultStyles).not.toContain("max-width: 1180px;");
  expect(cleanupStyles).toContain("repeat(auto-fit, minmax(min(100%, 15rem), 1fr))");
});
