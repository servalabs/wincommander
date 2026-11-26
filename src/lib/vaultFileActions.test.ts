import { describe, expect, test } from "bun:test";
import { resolveVaultFilePath, vaultKindFromContainer } from "./vaultFileActions";

describe("vault file targets", () => {
  const names = [
    { path: "D:\\Vaults\\sales.hc", name: "Sales" },
    { path: "D:\\Vaults\\a.hc", name: "Twin" },
    { path: "D:\\Vaults\\b.hc", name: "Twin" },
  ];

  test("a row's own path wins over any name match", () => {
    expect(resolveVaultFilePath({ path: " E:\\x.hc ", label: "Sales" }, names)).toBe("E:\\x.hc");
  });

  test("a row without a path uses the one named vault with its label", () => {
    expect(resolveVaultFilePath({ path: null, label: "Sales" }, names)).toBe("D:\\Vaults\\sales.hc");
  });

  test("an ambiguous or unknown label prefills nothing rather than a guess", () => {
    expect(resolveVaultFilePath({ path: null, label: "Twin" }, names)).toBe("");
    expect(resolveVaultFilePath({ path: null, label: "Other" }, names)).toBe("");
    expect(resolveVaultFilePath({ path: null }, names)).toBe("");
  });

  test("a dual container is a decoy vault", () => {
    expect(vaultKindFromContainer("dual")).toBe("decoy");
    expect(vaultKindFromContainer("standard")).toBe("standard");
  });
});
