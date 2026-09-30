import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const backendHook = readFileSync("src/hooks/useBackend.ts", "utf8");
const freeBackend = readFileSync("src-tauri/commander-free/src/backend.rs", "utf8");

const interfaceBody = (name: string) => {
  const match = backendHook.match(new RegExp(`export interface ${name} \\{([\\s\\S]*?)\\n\\}`));
  expect(match).not.toBeNull();
  return match?.[1] ?? "";
};

test("snapshot IPC distinguishes attach, restore, and refresh from the legacy empty-container commands", () => {
  for (const command of ["Attach-StegoContainer", "Restore-StegoContainer", "Refresh-StegoContainer"]) {
    expect(backendHook).toMatch(new RegExp(`execute(?:<[^>]+>)?\\(\\"${command}\\"`));
    expect(freeBackend).toContain(`"${command}"`);
  }
  expect(backendHook).toMatch(/execute(?:<[^>]+>)?\("Create-StegoMp4"/);
  expect(backendHook).toContain('execute("Extract-StegoMp4"');
});

test("attach sends an opaque existing container and never accepts a password", () => {
  const body = interfaceBody("AttachStegoContainerParams");
  expect(body).toContain("carrierPath: string");
  expect(body).toContain("containerPath: string");
  expect(body).toContain("outputPath?: string");
  expect(body).toContain("replaceExisting?: boolean");
  expect(body.toLowerCase()).not.toContain("password");

  const call = backendHook.match(/attachStegoContainer:[\s\S]*?\n\s*\}\),/);
  expect(call?.[0]).toContain("CarrierPath: params.carrierPath");
  expect(call?.[0]).toContain("ContainerPath: params.containerPath");
  expect(call?.[0]).toContain("ReplaceExisting: params.replaceExisting === true");
  expect(call?.[0]).not.toContain("Password");
});

test("restore supplies a destination folder only, and requests an original-name result", () => {
  const body = interfaceBody("RestoreStegoContainerParams");
  expect(body).toContain("inputPath: string");
  expect(body).toContain("destinationDir: string");
  expect(body).not.toContain("outputPath: string");
  expect(backendHook).toContain('execute<{ outputPath: string }>("Restore-StegoContainer"');
  expect(backendHook).toContain("DestinationDir: params.destinationDir");
});

test("refresh makes replacement explicit before the protected backend can replace a snapshot", () => {
  const body = interfaceBody("RefreshStegoContainerParams");
  expect(body).toContain("backupVideoPath: string");
  expect(body).toContain("containerPath: string");
  expect(body).toContain("replaceExisting: true");
  expect(backendHook).toContain("BackupVideoPath: params.backupVideoPath");
  expect(backendHook).toContain("ReplaceExisting: params.replaceExisting === true");
});
