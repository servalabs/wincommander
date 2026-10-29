import { expect, test } from "bun:test";
import { preserveDashboardPolicyUnknowns } from "./dashboardPolicyObservation";

test("unknown readback clears cached success for dashboard policies and DMA hardware state", () => {
  const patch = preserveDashboardPolicyUnknowns({}, {
    recallSnapshotsDisabled: null, internetCommRestricted: null,
    officeLoggingDisabled: null, bitlockerAutoEncryptDisabled: null,
    kernelDmaProtect: null,
  });
  expect(patch).toEqual({ current: {
    privacy: { tracking: { recallSnapshotsDisabled: null, officeLoggingDisabled: null }, internetCommunication: { restrictedEnabled: null } },
    tweaks: { security: { bitlockerAutoEncryptDisabled: null, kernelDmaProtect: null } },
  } });
});

test("a probe which did not request a policy does not erase its cached observation", () => {
  const patch = { current: { privacy: { tracking: { recallSnapshotsDisabled: true } } } };
  expect(preserveDashboardPolicyUnknowns(patch, undefined)).toEqual(patch);
  expect(preserveDashboardPolicyUnknowns({}, { officeLoggingDisabled: false })).toEqual({});
  expect(preserveDashboardPolicyUnknowns({}, {})).toEqual({});
});


test("unknown location clears the cached current observation without changing intent", () => {
  const patch = preserveDashboardPolicyUnknowns({ current: { privacy: { telemetry: { locationTrackingDisabled: true } } } }, { locationTrackingDisabled: null });
  expect(patch).toEqual({ current: { privacy: { telemetry: { locationTrackingDisabled: null } } } });
  expect(preserveDashboardPolicyUnknowns({}, { locationTrackingDisabled: false })).toEqual({});
});
