import { expect, test } from "bun:test";
import { buildMountVolumeRequest } from "../../hooks/useBackend";

test("mount sends distinct non-default PIM values for outer and hidden volumes", () => {
  const request = buildMountVolumeRequest({
    volumePath: "C:\\Vaults\\outer.hc",
    driveLetter: "V",
    password: "outer-test-password",
    keyfiles: ["C:\\Vaults\\outer.key"],
    pim: "24680",
    protectHidden: true,
    hiddenPassword: "hidden-test-password",
    hiddenKeyfiles: ["C:\\Vaults\\hidden.key"],
    hiddenPim: "13579",
    scope: "per-user",
  });

  expect(request).toEqual({
    VolumePath: "C:\\Vaults\\outer.hc",
    DriveLetter: "V",
    Password: "outer-test-password",
    Keyfiles: JSON.stringify(["C:\\Vaults\\outer.key"]),
    Pim: "24680",
    ReadOnly: false,
    Removable: false,
    ProtectHidden: true,
    HiddenPassword: "hidden-test-password",
    HiddenKeyfiles: JSON.stringify(["C:\\Vaults\\hidden.key"]),
    HiddenPim: "13579",
    Scope: "per-user",
    HardenAcl: true,
  });
});
