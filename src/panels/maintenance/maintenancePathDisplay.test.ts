import { describe, expect, test } from "bun:test";
import { toExplorerDisplayPath } from "./maintenancePathDisplay";

describe("toExplorerDisplayPath", () => {
  test("removes a drive extended-length prefix for display only", () => {
    expect(toExplorerDisplayPath("\\\\?\\C:\\Users\\Zoë\\App.lnk"))
      .toBe("C:\\Users\\Zoë\\App.lnk");
  });

  test("converts an extended UNC path to Explorer form", () => {
    expect(toExplorerDisplayPath("\\\\?\\UNC\\server\\share\\Cache\\item.bin"))
      .toBe("\\\\server\\share\\Cache\\item.bin");
  });

  test("keeps full-width Unicode and ordinary paths unchanged", () => {
    expect(toExplorerDisplayPath("D:\\資料\\Café\\cache.bin"))
      .toBe("D:\\資料\\Café\\cache.bin");
  });

  test("refuses lossy or unsupported namespace paths", () => {
    expect(toExplorerDisplayPath("D:\\bad?name\\cache.bin")).toBeNull();
    expect(toExplorerDisplayPath("D:\\bad\uFFFDname\\cache.bin")).toBeNull();
    expect(toExplorerDisplayPath("\\\\?\\Volume{guid}\\cache.bin")).toBeNull();
  });
});
