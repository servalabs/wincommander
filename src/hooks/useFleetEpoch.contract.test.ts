import { expect, test } from "bun:test";

test("Fleet desktop reconciliation runs once; native owns the recurring heartbeat", async () => {
  const source = await Bun.file("src/hooks/useFleetEpoch.ts").text();

  expect(source).toContain("void apply();");
  expect(source).toContain("The native process owns the recurring 60-second policy heartbeat");
  expect(source).not.toContain("setInterval(");
  expect(source).not.toContain("POLL_MS = 2_000");
});
