import { expect, test } from "bun:test";
import { preserveDashboardPolicyUnknowns } from "./dashboardPolicyObservation";

test("unknown readback clears cached success for all four dashboard policies", () => {
  const patch = preserveDashboardPolicyUnknowns({}, {
    recallSnapshotsDisabled: null, internetCommRestricted: null,
    officeLoggingDisabled: null, bitlockerAutoEncryptDisabled: null,
  });
  expect(patch).toEqual({ current: {
    privacy: { tracking: { recallSnapshotsDisabled: null, officeLoggingDisabled: null }, internetCommunication: { restrictedEnabled: null } },
    tweaks: { security: { bitlockerAutoEncryptDisabled: null } },
  } });
});

test("a probe which did not request a policy does not erase its cached observation", () => {
  const patch = { current: { privacy: { tracking: { recallSnapshotsDisabled: true } } } };
  expect(preserveDashboardPolicyUnknowns(patch, undefined)).toEqual(patch);
  expect(preserveDashboardPolicyUnknowns({}, { officeLoggingDisabled: false })).toEqual({});
  expect(preserveDashboardPolicyUnknowns({}, {})).toEqual({});
});
