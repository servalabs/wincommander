import { describe, expect, test } from "bun:test";
import {
  DESTRUCT_STEPS,
  DESTRUCT_STEP_DESCRIPTIONS,
} from "./lockdownSteps";

const NYX_FULL_SCOPE_ADVANCED_IDS = [
  "nyx_all_user_certificates",
  "nyx_all_scheduled_tasks",
  "nyx_all_services",
  "nyx_all_wireless_profiles",
  "nyx_all_vpn_connections",
  "nyx_all_chrome_extensions",
  "nyx_all_crypto_provider_data",
  "nyx_windows_update_log",
  "nyx_push_notifications",
] as const;

describe("Nyx full-scope advanced cleanup steps", () => {
  test("remain opt-in Deep Trace actions with an operator-facing impact explanation", () => {
    for (const id of NYX_FULL_SCOPE_ADVANCED_IDS) {
      const step = DESTRUCT_STEPS.find((candidate) => candidate.id === id);
      expect(step, `${id} must remain selectable`).toBeDefined();
      expect(step?.group).toBe("deepDfir");
      expect(step?.defaultEnabled).toBeFalse();

      const description = DESTRUCT_STEP_DESCRIPTIONS[id];
      expect(description, `${id} must explain its impact`).toBeTruthy();
      expect(description.length).toBeGreaterThan(70);
    }
  });
});
