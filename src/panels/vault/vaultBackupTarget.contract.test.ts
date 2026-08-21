import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/panels/vault/index.tsx", "utf8");

test("emergency backup registration is shown only when it can apply", () => {
  expect(source).toContain("{paid && volumes.length === 1 && (");
});
