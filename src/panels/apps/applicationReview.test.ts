import { expect, test } from "bun:test";
import { programRemovalUnavailable } from "./applicationReview";
import { formatSize } from "./components/ApplicationReviewResults";
import type { BcuApplication } from "../../hooks/useBackend";

const app = { displayName: "Example", isProtected: false, isSystemComponent: false } as BcuApplication;
test("program selection refuses protected, ambiguous and comma-split uninstall identities", () => {
  expect(programRemovalUnavailable(app, [app])).toBeUndefined();
  expect(programRemovalUnavailable({ ...app, isProtected: true }, [app])).toContain("Protected");
  expect(programRemovalUnavailable({ ...app, isSystemComponent: true }, [app])).toContain("Protected");
  expect(programRemovalUnavailable(app, [app, { ...app, displayName: "EXAMPLE" }])).toContain("Multiple programs");
  expect(programRemovalUnavailable({ ...app, displayName: "Example, Editor" }, [])).toContain("cannot be targeted safely");
});
test("missing and invalid installed sizes remain unavailable, with accurate binary units", () => {
  for (const size of [undefined, 0, -1, Number.NaN, Number.POSITIVE_INFINITY]) expect(formatSize(size)).toBe("Size unavailable");
  expect(formatSize(10)).toBe("10 KiB");
  expect(formatSize(1024)).toBe("1.0 MiB");
  expect(formatSize(1024 * 1024)).toBe("1.0 GiB");
});
