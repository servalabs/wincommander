import { existsSync } from "node:fs";
import { isAbsolute, resolve } from "node:path";

export const PRO_WORKSPACE_ENV = "WINCOMMANDER_PRO_WORKSPACE";

const PRO_WORKSPACE_CANDIDATES = [
  "commander-pro",
  "wicommander-pro",
  "wincommander-pro",
];

function resolveOverridePath(rootDir: string, override: string): string {
  return isAbsolute(override) ? override : resolve(rootDir, override);
}

function hasCargoManifest(workspaceDir: string): boolean {
  return existsSync(resolve(workspaceDir, "Cargo.toml"));
}

export function findProWorkspace(
  rootDir: string,
  env: NodeJS.ProcessEnv = process.env,
): string {
  const overrideWorkspace = getOverrideWorkspacePath(rootDir, env);
  if (overrideWorkspace) return overrideWorkspace;

  for (const workspaceDir of getProWorkspaceCandidates(rootDir)) {
    if (hasCargoManifest(workspaceDir)) return workspaceDir;
  }

  throw new Error(
    `Set ${PRO_WORKSPACE_ENV} or clone the Pro workspace as ../commander-pro, ../wicommander-pro, or ../wincommander-pro.`,
  );
}

export function getProManifestPath(
  rootDir: string,
  env: NodeJS.ProcessEnv = process.env,
): string {
  return resolve(findProWorkspace(rootDir, env), "Cargo.toml");
}

export function getProWorkspaceCandidates(rootDir: string): string[] {
  return PRO_WORKSPACE_CANDIDATES.map((candidate) => resolve(rootDir, "..", candidate));
}

export function getOverrideWorkspacePath(
  rootDir: string,
  env: NodeJS.ProcessEnv = process.env,
): string | null {
  const override = env[PRO_WORKSPACE_ENV]?.trim();
  if (!override) return null;
  const workspaceDir = resolveOverridePath(rootDir, override);
  if (!hasCargoManifest(workspaceDir)) {
    throw new Error(
      `${PRO_WORKSPACE_ENV} points to "${workspaceDir}", but Cargo.toml was not found there.`,
    );
  }
  return workspaceDir;
}
