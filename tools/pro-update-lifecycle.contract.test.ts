import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

test("Pro replacement drains temporarily while app exit remains terminal", () => {
    const source = readFileSync("src-tauri/commander-free/src/pro_install.rs", "utf8");
    const installer = source.slice(source.indexOf("async fn install_pro_binary_machine("));
    const sidecar = readFileSync("src-tauri/commander-free/src/sidecar.rs", "utf8");
    expect(installer).toContain("drain_pro_sessions_for_update(&_maintenance).await");
    expect(installer).not.toContain("crate::sidecar::close_pro_session().await");
    const maintenance = installer.indexOf("let _maintenance = update_guard::Maintenance::begin");
    expect(maintenance).toBeLessThan(installer.indexOf("drain_pro_sessions_for_update(&_maintenance)"));
    expect(sidecar).toContain("pub async fn close_pro_session() {\n    shutdown_signal().send_replace(true);");
    const temporaryDrain = sidecar.slice(
        sidecar.indexOf("pub(crate) async fn drain_pro_sessions_for_update("),
        sidecar.indexOf("async fn drain_pro_sessions_bounded()"),
    );
    expect(temporaryDrain).toContain("&crate::pro_install::update_guard::Maintenance");
    expect(temporaryDrain).toContain("drain_pro_sessions_bounded().await");
    expect(temporaryDrain).not.toContain("send_replace");
    expect(sidecar).not.toContain("shutdown_signal().send_replace(false)");
});
