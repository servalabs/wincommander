import { describe, expect, test } from "bun:test";

declare const Bun: { file(path: URL): { text(): Promise<string> } };

describe("automatic Pro update entry points", () => {
  test("clears an earlier Pro result before starting another update flow", async () => {
    const flow = await Bun.file(new URL("./useUpdateFlow.ts", import.meta.url)).text();
    const start = flow.slice(flow.indexOf("const start = useCallback"), flow.indexOf("const retryFree"));
    expect(start).toContain("resetProInstall();");
    expect(start.indexOf("resetProInstall();")).toBeLessThan(start.indexOf('setPhase("checking-free")'));
  });

  test("guards both the Pro-only trigger and the combined Free-to-Pro flow", async () => {
    const automatic = await Bun.file(new URL("./useAutomaticUpdate.ts", import.meta.url)).text();
    const flow = await Bun.file(new URL("./useUpdateFlow.ts", import.meta.url)).text();
    expect(automatic).toContain("if (!shouldAutomaticallyReplacePro(pro.status, pro.manifest)) return;");
    expect(flow).toContain("automaticProInstallConsent === false && !shouldAutomaticallyReplacePro(pro.status, pro.manifest)");
    expect(flow).toContain("if (automaticProInstallConsent === false && pro.manifest && !shouldAutomaticallyReplacePro(pro.status, pro.manifest)) {");
  });
});
