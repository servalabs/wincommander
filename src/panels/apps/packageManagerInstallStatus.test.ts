import { describe, expect, test } from "bun:test";
import { summarizeOptionalManagerInstall } from "./packageManagerInstallStatus";

describe("optional package manager install status", () => {
  test("keeps partial success and clearly identifies a Scoop failure", () => {
    const status = summarizeOptionalManagerInstall({
      installed: ["chocolatey"],
      alreadyInstalled: [],
      errors: ["scoop: installation did not complete; installer was still in use"],
    });

    expect(status.tone).toBe("warning");
    expect(status.text).toBe("Installed Chocolatey. Scoop could not be installed: installation did not complete; installer was still in use");
  });

  test("does not report success if the backend confirms no installation", () => {
    const status = summarizeOptionalManagerInstall({
      installed: [],
      alreadyInstalled: [],
      errors: [],
    });

    expect(status.tone).toBe("warning");
    expect(status.text).toContain("No package manager installation was confirmed");
  });

  test("reports successful and already-installed managers as completed", () => {
    const status = summarizeOptionalManagerInstall({
      installed: ["scoop"],
      alreadyInstalled: ["chocolatey"],
      errors: [],
    });

    expect(status.tone).toBe("success");
    expect(status.text).toBe("Installed Scoop. Chocolatey already installed.");
  });
});
