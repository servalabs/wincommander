// Changed-file quality guardrails. These deliberately inspect only additions
// since a baseline: existing debt is not a reason to block an unrelated fix.
//
// The two rules are intentionally small and objective:
//   1. no new explicit `any` in TS/TSX (unless a same-line documented waiver);
//   2. no new source file over 320 lines, or a small file grown beyond 360.
//
// Run locally with `bun run lint:quality`. CI supplies QUALITY_GUARD_BASE so
// every commit in a pull request is inspected, not merely its final commit.

import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const SOURCE_EXTENSIONS = new Set([".ts", ".tsx", ".rs", ".ps1", ".css"]);
const MAX_NEW_FILE_LINES = 320;
const MAX_SMALL_FILE_LINES = 360;
const EXPLICIT_ANY = new RegExp([
  "\\bas\\s+any\\b",
  ":\\s*any(?:\\[\\])?(?=\\s*[,)=;={])",
  "<" + "any>",
].join("|"));
const WAIVER = "quality-guard: allow-explicit-any";

type DiffAddition = { path: string; line: string; lineNumber: number };

function git(args: string[]): string {
  const result = spawnSync("git", args, { cwd: ROOT, encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || `git ${args.join(" ")} failed`);
  }
  return result.stdout;
}

function isSource(path: string): boolean {
  const dot = path.lastIndexOf(".");
  return dot !== -1 && SOURCE_EXTENSIONS.has(path.slice(dot));
}

function resolveBaseline(): string {
  const supplied = process.env.QUALITY_GUARD_BASE;
  if (supplied && !/^0+$/.test(supplied)) {
    const exists = spawnSync("git", ["cat-file", "-e", `${supplied}^{commit}`], { cwd: ROOT });
    if (exists.status === 0) return supplied;
  }
  try {
    return git(["merge-base", "HEAD", "origin/main"]).trim();
  } catch {
    try {
      return git(["rev-parse", "HEAD^"]).trim();
    } catch {
      return "4b825dc642cb6eb9a060e54bf8d69288fbee4904"; // empty tree
    }
  }
}

function changedAdditions(base: string): DiffAddition[] {
  const diff = git(["diff", "--no-ext-diff", "--unified=0", base, "--"]);
  const additions: DiffAddition[] = [];
  let path: string | null = null;
  let lineNumber = 0;

  for (const line of diff.split("\n")) {
    if (line.startsWith("+++ b/")) {
      path = line.slice(6);
      continue;
    }
    if (line.startsWith("@@ ")) {
      const match = /\+(\d+)/.exec(line);
      lineNumber = match ? Number(match[1]) : 0;
      continue;
    }
    if (!path || !isSource(path)) continue;
    if (line.startsWith("+") && !line.startsWith("+++")) {
      additions.push({ path, line: line.slice(1), lineNumber });
      lineNumber++;
    } else if (!line.startsWith("-")) {
      lineNumber++;
    }
  }
  return additions;
}

function untrackedSourceFiles(): string[] {
  return git(["ls-files", "--others", "--exclude-standard"])
    .split("\n")
    .filter((path) => path && isSource(path));
}

function lineCount(path: string): number {
  const absolute = resolve(ROOT, path);
  return existsSync(absolute) ? readFileSync(absolute, "utf8").split("\n").length : 0;
}

function baselineExists(base: string, path: string): boolean {
  return spawnSync("git", ["cat-file", "-e", `${base}:${path}`], { cwd: ROOT }).status === 0;
}

function main(): void {
  const base = resolveBaseline();
  const additions = changedAdditions(base);
  const violations: string[] = [];

  for (const addition of additions) {
    const code = addition.line.trim();
    if (code.startsWith("//") || code.startsWith("*")) continue;
    if (EXPLICIT_ANY.test(code) && !code.includes(WAIVER)) {
      violations.push(
        `${addition.path}:${addition.lineNumber} adds explicit any; use a concrete type, unknown plus validation, or a same-line '${WAIVER}: <reason>' waiver.`,
      );
    }
  }

  for (const path of untrackedSourceFiles()) {
    const lines = readFileSync(resolve(ROOT, path), "utf8").split("\n");
    for (const [index, line] of lines.entries()) {
      const code = line.trim();
      if (code.startsWith("//") || code.startsWith("*")) continue;
      if (EXPLICIT_ANY.test(code) && !code.includes(WAIVER)) {
        violations.push(
          `${path}:${index + 1} adds explicit any; use a concrete type, unknown plus validation, or a same-line '${WAIVER}: <reason>' waiver.`,
        );
      }
    }
  }

  const changedFiles = new Set(additions.map((addition) => addition.path));
  for (const path of untrackedSourceFiles()) changedFiles.add(path);
  for (const path of changedFiles) {
    const lines = lineCount(path);
    const existed = baselineExists(base, path);
    if (!existed && lines > MAX_NEW_FILE_LINES) {
      violations.push(`${path} is a new ${lines}-line source file (limit ${MAX_NEW_FILE_LINES}); split it by responsibility.`);
    }
    if (existed && lines > MAX_SMALL_FILE_LINES) {
      const oldLines = Number(git(["show", `${base}:${path}`]).split("\n").length);
      if (oldLines <= MAX_NEW_FILE_LINES) {
        violations.push(`${path} grew from ${oldLines} to ${lines} lines (limit ${MAX_SMALL_FILE_LINES}); extract the new responsibility.`);
      }
    }
  }

  if (violations.length > 0) {
    console.error(`[quality-guards] FAIL — ${violations.length} changed-file violation(s):`);
    for (const violation of violations) console.error(`  - ${violation}`);
    process.exit(1);
  }

  console.log(
    `[quality-guards] OK — checked ${new Set(additions.map((addition) => addition.path)).size} changed source file(s) against ${base.slice(0, 12)}.`,
  );
}

main();
