import { afterEach, describe, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
  findProWorkspace,
  getProManifestPath,
  PRO_WORKSPACE_ENV,
} from "./pro-workspace";

const tempDirs: string[] = [];

function createSandbox() {
  const sandboxDir = mkdtempSync(join(tmpdir(), "wincommander-pro-workspace-"));
  const rootDir = join(sandboxDir, "wincommander");
  mkdirSync(rootDir, { recursive: true });
  tempDirs.push(sandboxDir);
  return { rootDir, sandboxDir };
}

function addWorkspace(sandboxDir: string, name: string): string {
  const workspaceDir = join(sandboxDir, name);
  mkdirSync(workspaceDir, { recursive: true });
  writeFileSync(join(workspaceDir, "Cargo.toml"), "[workspace]\n");
  return workspaceDir;
}

afterEach(() => {
  while (tempDirs.length > 0) {
    rmSync(tempDirs.pop()!, { recursive: true, force: true });
  }
});

describe("findProWorkspace", () => {
  test("prefers WINCOMMANDER_PRO_WORKSPACE when it points at a valid Cargo workspace", () => {
    const { rootDir, sandboxDir } = createSandbox();
    const overrideWorkspace = addWorkspace(sandboxDir, "custom-pro");

    expect(
      findProWorkspace(rootDir, {
        [PRO_WORKSPACE_ENV]: "../custom-pro",
      }),
    ).toBe(overrideWorkspace);
  });

  test("uses the canonical commander-pro sibling when present", () => {
    const { rootDir, sandboxDir } = createSandbox();
    const workspaceDir = addWorkspace(sandboxDir, "commander-pro");

    expect(findProWorkspace(rootDir, {})).toBe(workspaceDir);
    expect(getProManifestPath(rootDir, {})).toBe(resolve(workspaceDir, "Cargo.toml"));
  });

  test("falls back to wicommander-pro for legacy local checkouts", () => {
    const { rootDir, sandboxDir } = createSandbox();
    const workspaceDir = addWorkspace(sandboxDir, "wicommander-pro");

    expect(findProWorkspace(rootDir, {})).toBe(workspaceDir);
  });

  test("mentions all supported locations when no Pro workspace is available", () => {
    const { rootDir } = createSandbox();

    expect(() => findProWorkspace(rootDir, {})).toThrow(
      `Set ${PRO_WORKSPACE_ENV} or clone the Pro workspace as ../commander-pro, ../wicommander-pro, or ../wincommander-pro.`,
    );
  });
});
