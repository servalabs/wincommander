import { describe, expect, test } from "bun:test";
import { toExplorerDisplayPath } from "./windowsPathDisplay";

describe("toExplorerDisplayPath", () => {
  test("uses Explorer-readable drive and UNC locations", () => {
    expect(toExplorerDisplayPath("\\\\?\\C:\\Users\\Parth\\App")).toBe("C:\\Users\\Parth\\App");
    expect(toExplorerDisplayPath("\\\\?\\UNC\\server\\share\\App")).toBe("\\\\server\\share\\App");
  });

  test("does not display unsupported or lossy paths as a guess", () => {
    expect(toExplorerDisplayPath("\\\\?\\Volume{guid}\\App")).toBeNull();
    expect(toExplorerDisplayPath("C:\\Users\\?\\App")).toBeNull();
  });
});
