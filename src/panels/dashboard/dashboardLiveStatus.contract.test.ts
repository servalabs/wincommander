import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

test("Dashboard primes its live DNS readout on mount", () => {
  const sourcePath = new URL("./index.tsx", import.meta.url).pathname.replace(
    /^\/([A-Za-z]:\/)/,
    "$1",
  );
  const source = readFileSync(sourcePath, "utf8");

  expect(source).toContain("void refreshNetwork(true);");
  expect(source).toContain("}, [refreshNetwork]);");
});

test("Fix All keeps its all-users scope opt-in and explicit", () => {
  const sourcePath = new URL("./index.tsx", import.meta.url).pathname.replace(
    /^\/([A-Za-z]:\/)/,
    "$1",
  );
  const source = readFileSync(sourcePath, "utf8");

  expect(source).toContain("applyFixAllMachineWide === true");
  expect(source).toContain("MachineWide: machineWide");
  expect(source).toContain("if (machineWide && needsElevation)");
  expect(source).toContain("Apply Fix All to all users");
});
