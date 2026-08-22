import { expect, test } from "bun:test";
import { buildMountVolumeRequest } from "../../hooks/useBackend";

test("outer-decoy mount sends distinct non-default PIM values and its exact role", () => {
  const request = buildMountVolumeRequest({
    volumePath: "C:\\Vaults\\outer.hc",
    driveLetter: "V",
    volumeKind: "dual",
    volumeRole: "outer",
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
    VolumeKind: "dual",
    VolumeRole: "outer",
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

test("hidden-volume mount sends the hidden role through the primary credentials", () => {
  expect(buildMountVolumeRequest({
    volumePath: "C:\\Vaults\\dual.hc",
    driveLetter: "H",
    volumeKind: "dual",
    volumeRole: "hidden",
    password: "hidden-test-password",
    pim: "13579",
    scope: "per-user",
  })).toEqual({
    VolumePath: "C:\\Vaults\\dual.hc",
    DriveLetter: "H",
    VolumeKind: "dual",
    VolumeRole: "hidden",
    Password: "hidden-test-password",
    Keyfiles: "[]",
    Pim: "13579",
    ReadOnly: false,
    Removable: false,
    ProtectHidden: false,
    HiddenKeyfiles: "[]",
    Scope: "per-user",
    HardenAcl: true,
  });
});
