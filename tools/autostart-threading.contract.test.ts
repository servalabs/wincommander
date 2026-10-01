import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

test("startup integrity runs once and Windows task commands leave the event thread", () => {
    const app = readFileSync("src/App.tsx", "utf8");
    const native = readFileSync("src-tauri/commander-free/src/lib.rs", "utf8");
    const commands = readFileSync("src-tauri/commander-free/src/autostart.rs", "utf8");
    expect(app).not.toContain('invoke("ensure_autostart_task")');
    expect(native).toContain("std::thread::spawn(move || {\n                    let succeeded = autostart::ensure_autostart_task_sync().is_ok()");
    for (const name of ["ensure_autostart_task", "enable_autostart_task", "remove_autostart_task", "update_autostart_task_identity", "is_autostart_enabled"]) {
        const start = commands.indexOf(`pub async fn ${name}(`);
        expect(start).toBeGreaterThan(-1);
        const body = commands.slice(start, commands.indexOf("\n}\n", start));
        expect(body).toContain("run_autostart_off_thread(");
    }
    expect(commands).toContain("tauri::async_runtime::spawn_blocking(operation)");
    expect(commands).toContain("WinCommander autostart worker failed:");
});
