import { describe, expect, test } from "bun:test";

const source = await Bun.file("src-tauri/commander-free/src/fleet_agent.rs").text();
const connectRegion = source.slice(source.indexOf("pub async fn fleet_connect("), source.indexOf("pub async fn fleet_status("));
// fleet_set_dispatch is a separate command whose ON-failure rollback legitimately patches settings.
const setDispatchStart = connectRegion.indexOf("pub async fn fleet_set_dispatch(");
const setDispatchEnd = connectRegion.indexOf("pub async fn fleet_machine_trust_status(");
const connect = connectRegion.slice(0, setDispatchStart) + connectRegion.slice(setDispatchEnd);

describe("native Fleet enrollment boundary", () => {
  test("authority reads fail closed before a plan is created", () => {
    expect(connect).toContain("let current = crate::settings::read_settings()?;");
    expect(connect).toContain("ConnectionPlan::new(");
    expect(connect).not.toContain("if let Ok(prev)");
    expect(connect).not.toContain("unwrap_or_default()");
  });

  test("approval is request-bound and a commit rechecks its snapshot", () => {
    expect(connect).toContain("plan.confirmation_binding()");
    expect(connect).toContain("plan.commit(confirmed)?");
    expect(connect).toMatch(/FLEET_CONNECT_GATE\s*\.try_lock\(\)/);
    expect(connect).not.toContain("settings::patch_settings(");
  });

  test("the sidecar receives the committed identity and connection", () => {
    for (const field of ["server_url", "dispatch", "signing_key_pub"]) {
      expect(connect).toContain(`committed.app.fleet.${field}`);
    }
    expect(connect).toContain("committed.device_id");
    expect(connect).not.toContain('"fleetMonitoringEnabled": false');
  });
});
