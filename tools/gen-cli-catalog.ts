// SPDX-License-Identifier: AGPL-3.0-or-later
// Generate the machine-readable command catalog consumed by wincommander-cli.
// The source of truth remains the real Tauri handler list, backend dispatcher,
// and frontend call sites; this file only normalizes those sources.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { relative, resolve } from "node:path";

type Transport = "tauri" | "backend-script";

interface CommandEntry {
  id: string;
  name: string;
  transport: Transport;
  registered: boolean;
  frontendReferences: string[];
}

interface CommandCatalog {
  schemaVersion: 1;
  commands: CommandEntry[];
}

const ROOT = resolve(import.meta.dir, "..");
const LIB_RS = resolve(ROOT, "src-tauri/commander-free/src/lib.rs");
const BACKEND_RS = resolve(ROOT, "src-tauri/commander-free/src/backend.rs");
const OUT = resolve(ROOT, "src-tauri/commander-free/src/cli_catalog.generated.json");
const INTERNAL_TAURI_HANDLERS = new Set(["mark_tauri_cli_ready", "complete_tauri_cli"]);

function normalizePath(path: string): string {
  return relative(ROOT, path).replaceAll("\\", "/");
}

function extractHandlerNames(source: string): Set<string> {
  const marker = ".invoke_handler(tauri::generate_handler![";
  const start = source.indexOf(marker);
  if (start < 0) throw new Error("Tauri generate_handler list not found");
  const end = source.indexOf("        ])", start);
  if (end < 0) throw new Error("Tauri generate_handler list terminator not found");
  const block = source.slice(start + marker.length, end);
  const names = new Set<string>();
  for (const line of block.split(/\r?\n/)) {
    const match = line.match(/^\s*(?:[A-Za-z_][\w]*::)*([A-Za-z_][\w]*),\s*$/);
    if (match) names.add(match[1]);
  }
  return names;
}

function functionBody(source: string, signature: string): string {
  const start = source.indexOf(signature);
  if (start < 0) throw new Error(`Function not found: ${signature}`);
  const open = source.indexOf("{", start + signature.length);
  if (open < 0) throw new Error(`Function body not found: ${signature}`);
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") {
      depth -= 1;
      if (depth === 0) return source.slice(open + 1, index);
    }
  }
  throw new Error(`Unterminated function body: ${signature}`);
}

function extractBackendNames(source: string): Set<string> {
  const names = new Set<string>();
  const dispatcher = functionBody(source, "fn get_module_for_command(command: &str)");
  const tierGate = functionBody(source, "pub(crate) fn get_command_tier(command: &str)");
  const executableSurface = `${dispatcher}\n${tierGate}`;
  // Only dispatcher literals are executable through run_backend_script. This
  // avoids turning unrelated UI labels, test cases, and error text into fake
  // commands simply because they happen to have Verb-Noun-like spelling.
  for (const match of executableSurface.matchAll(/"([A-Z][A-Za-z0-9_]*(?:-[A-Za-z0-9_$]+)+)"/g)) {
    names.add(match[1]);
  }
  // Sensitive paid command names are intentionally split with `~` in Rust.
  // The catalog reconstructs them logically, while canonical() JSON-escapes
  // every command separator so no full Verb-Noun name lands contiguously in
  // the shipped Free executable.
  for (const match of source.matchAll(/parts:\s*&\[((?:\s*"[^"]+"\s*,?)+)\]/g)) {
    const parts = [...match[1].matchAll(/"([^"]+)"/g)].map((part) => part[1].replaceAll("~", ""));
    if (parts.length) names.add(parts.join(""));
  }
  for (const match of executableSurface.matchAll(/matches_parts\(command,\s*&\[((?:\s*"[^"]+"\s*,?)+)\]\)/g)) {
    const parts = [...match[1].matchAll(/"([^"]+)"/g)].map((part) => part[1].replaceAll("~", ""));
    if (parts.length) names.add(parts.join(""));
  }
  return names;
}

async function frontendSources(): Promise<Array<{ path: string; source: string }>> {
  const files: Array<{ path: string; source: string }> = [];
  const glob = new Bun.Glob("src/**/*.{ts,tsx}");
  for await (const path of glob.scan({ cwd: ROOT, absolute: true, onlyFiles: true })) {
    const normalized = normalizePath(path);
    if (/\.(?:test|spec)\.[^.]+$/.test(normalized) || normalized.startsWith("src/dev/")) continue;
    files.push({ path, source: readFileSync(path, "utf8") });
  }
  return files;
}

function addReference(map: Map<string, Set<string>>, name: string, path: string): void {
  const refs = map.get(name) ?? new Set<string>();
  refs.add(normalizePath(path));
  map.set(name, refs);
}

async function buildCatalog(): Promise<CommandCatalog> {
  const libSource = readFileSync(LIB_RS, "utf8");
  const backendSource = readFileSync(BACKEND_RS, "utf8");
  const handlers = extractHandlerNames(libSource);
  for (const internal of INTERNAL_TAURI_HANDLERS) handlers.delete(internal);
  const registeredBackendNames = extractBackendNames(backendSource);
  const backendNames = new Set(registeredBackendNames);
  const tauriRefs = new Map<string, Set<string>>();
  const backendRefs = new Map<string, Set<string>>();

  for (const file of await frontendSources()) {
    for (const match of file.source.matchAll(/\binvoke(?:<[^;]{0,500}?>)?\(\s*["']([a-z][a-z0-9_]*)["']/g)) {
      addReference(tauriRefs, match[1], file.path);
    }
    for (const match of file.source.matchAll(/\bexecuteBackendCommand(?:<[^;]{0,500}?>)?\(\s*["']([^"']+)["']/g)) {
      backendNames.add(match[1]);
      addReference(backendRefs, match[1], file.path);
    }
    for (const match of file.source.matchAll(/\b(?:enableCmd|disableCmd|statusCmd|command):\s*["']([A-Za-z][A-Za-z0-9_]*(?:-[A-Za-z0-9_$]+)+)["']/g)) {
      backendNames.add(match[1]);
      addReference(backendRefs, match[1], file.path);
    }
  }

  const tauriNames = new Set([...handlers, ...tauriRefs.keys()]);
  const commands: CommandEntry[] = [
    ...[...tauriNames].map((name): CommandEntry => ({
      id: `tauri:${name}`,
      name,
      transport: "tauri",
      registered: handlers.has(name),
      frontendReferences: [...(tauriRefs.get(name) ?? [])].sort(),
    })),
    ...[...backendNames].map((name): CommandEntry => ({
      id: `backend:${name}`,
      name,
      transport: "backend-script",
      registered: registeredBackendNames.has(name),
      frontendReferences: [...(backendRefs.get(name) ?? [])].sort(),
    })),
  ].sort((a, b) => a.id.localeCompare(b.id));

  return { schemaVersion: 1, commands };
}

function canonical(catalog: CommandCatalog): string {
  return `${JSON.stringify(catalog, null, 2).replaceAll("-", "\\u002d")}\n`;
}

const wanted = canonical(await buildCatalog());
if (process.argv.includes("--check")) {
  const current = existsSync(OUT) ? readFileSync(OUT, "utf8").replaceAll("\r\n", "\n") : "";
  if (current !== wanted) {
    console.error("CLI command catalog is stale. Run `bun run gen:cli-catalog` and commit the result.");
    process.exit(1);
  }
  console.log("CLI command catalog is current.");
} else {
  writeFileSync(OUT, wanted);
  const parsed = JSON.parse(wanted) as CommandCatalog;
  const tauriCount = parsed.commands.filter((entry) => entry.transport === "tauri").length;
  const backendCount = parsed.commands.length - tauriCount;
  console.log(`wrote ${normalizePath(OUT)} (${tauriCount} Tauri, ${backendCount} backend commands)`);
}
