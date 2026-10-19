import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

function assertUpdateLifecycle(source: string, sidecar: string) {
    source = source.replace(/\r\n?/g, "\n");
    sidecar = sidecar.replace(/\r\n?/g, "\n");
    const installer = source.slice(source.indexOf("async fn install_pro_binary_machine("));
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
}

const installerSource = readFileSync("src-tauri/commander-free/src/pro_install.rs", "utf8");
const sidecarSource = readFileSync("src-tauri/commander-free/src/sidecar.rs", "utf8");

for (const newline of ["\n", "\r\n"]) {
    test(`Pro replacement preserves shutdown semantics with ${JSON.stringify(newline)} source lines`, () => {
        const convert = (value: string) => value.replace(/\r\n?/g, "\n").replace(/\n/g, newline);
        assertUpdateLifecycle(convert(installerSource), convert(sidecarSource));
    });
}

test("update lifecycle contract rejects removal of the temporary drain", () => {
    expect(() => assertUpdateLifecycle(
        installerSource.replace("drain_pro_sessions_for_update(&_maintenance).await", "unrelated_work().await"),
        sidecarSource,
    )).toThrow();
});

test("update lifecycle contract rejects clearing the terminal shutdown signal", () => {
    expect(() => assertUpdateLifecycle(
        installerSource,
        sidecarSource.replace("shutdown_signal().send_replace(true);", "shutdown_signal().send_replace(false);"),
    )).toThrow();
});
