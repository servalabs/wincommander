import { describe, expect, test } from "bun:test";
import { ALL_TOGGLES } from "./index";
import { resolveToggleIcon } from "./toggleIcons";

describe("catalog toggle icons", () => {
  test("gives every catalog toggle card a semantic icon", () => {
    for (const toggle of ALL_TOGGLES) {
      expect(resolveToggleIcon(toggle)).not.toBe("help");
    }
  });
});
