import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/panels/mesh/index.tsx", "utf8");

describe("private mesh deep-state interaction contracts", () => {
  test("file selection is a named keyboard-operable control", () => {
    expect(source).toContain('className="mesh-file-dropzone group"');
    expect(source).toContain('type="button"');
    expect(source).toContain('disabled={fileTransferLoading}');
    expect(source).toContain('"Choose a file to send"');
  });
});
