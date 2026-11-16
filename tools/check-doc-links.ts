// tools/check-doc-links.ts
//
// Documentation link checker: verifies that every relative link and in-page or
// cross-file #anchor in the repository's Markdown files resolves. External
// (http/https/mailto) links are never fetched.
//
// Run via: bun run lint:docs
//   --json   print findings as JSON (for before/after comparisons)
//
// Scope: tracked-looking *.md files under the repository root, skipping
// node_modules, build output, the `assets` submodule, and vendored licences.
// Fenced code blocks and inline code spans are ignored. Anchors follow the
// GitHub heading-slug rules, including -1/-2 suffixes for duplicate headings.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const SKIP_DIRS = new Set([
  "node_modules",
  ".git",
  "target",
  "dist",
  "assets",
  "LICENSES",
  ".worktrees",
]);

function collect(dir: string, out: string[]): void {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (!SKIP_DIRS.has(name)) collect(full, out);
    } else if (name.toLowerCase().endsWith(".md")) {
      out.push(full);
    }
  }
}

function stripCode(text: string): string {
  // Blank out fenced code blocks (keeping line numbers) and inline code spans.
  const lines = text.split("\n");
  let fence: string | null = null;
  const kept = lines.map((line) => {
    const m = /^\s{0,3}(`{3,}|~{3,})/.exec(line);
    if (fence) {
      if (m && m[1][0] === fence[0] && m[1].length >= fence.length) fence = null;
      return "";
    }
    if (m) {
      fence = m[1];
      return "";
    }
    return line.replace(/`[^`\n]*`/g, (s) => " ".repeat(s.length));
  });
  return kept.join("\n");
}

function slug(heading: string): string {
  return heading
    .trim()
    .toLowerCase()
    .replace(/<[^>]+>/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[`*_~]/g, "")
    .replace(/[^\p{L}\p{N}\p{M}\- _]/gu, "")
    .replace(/ /g, "-");
}

const anchorCache = new Map<string, Set<string>>();
function anchorsOf(file: string): Set<string> {
  const cached = anchorCache.get(file);
  if (cached) return cached;
  const set = new Set<string>();
  const seen = new Map<string, number>();
  const text = stripCode(readFileSync(file, "utf8"));
  for (const line of text.split("\n")) {
    const m = /^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$/.exec(line);
    if (m) {
      const base = slug(m[1]);
      const n = seen.get(base) ?? 0;
      seen.set(base, n + 1);
      set.add(n === 0 ? base : `${base}-${n}`);
    }
    for (const a of line.matchAll(/<a\s+[^>]*(?:id|name)="([^"]+)"/gi)) set.add(a[1]);
  }
  anchorCache.set(file, set);
  return set;
}

interface Finding {
  file: string;
  line: number;
  target: string;
  problem: string;
}

const files: string[] = [];
collect(ROOT, files);
files.sort();
const findings: Finding[] = [];

const LINK_RES = [
  /!?\[(?:[^\]\\]|\\.)*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g, // inline
  /^\s{0,3}\[[^\]]+\]:\s*<?(\S+?)>?(?:\s+"[^"]*")?\s*$/gm, // reference definition
  /<(?:a|img)\s+[^>]*(?:href|src)="([^"]+)"/gi, // html
];

for (const file of files) {
  const text = stripCode(readFileSync(file, "utf8"));
  const rel = relative(ROOT, file).split(sep).join("/");
  for (const re of LINK_RES) {
    re.lastIndex = 0;
    for (const m of text.matchAll(re)) {
      const target = m[1];
      if (/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(target)) continue; // external
      const line = text.slice(0, m.index).split("\n").length;
      const hash = target.indexOf("#");
      const pathPart = decodeURIComponent(hash === -1 ? target : target.slice(0, hash));
      const anchor = hash === -1 ? "" : decodeURIComponent(target.slice(hash + 1));
      const dest = pathPart === "" ? file : resolve(dirname(file), pathPart);
      if (!dest.startsWith(ROOT)) {
        findings.push({ file: rel, line, target, problem: "escapes repository" });
        continue;
      }
      if (!existsSync(dest)) {
        findings.push({ file: rel, line, target, problem: "missing file" });
        continue;
      }
      if (anchor && statSync(dest).isFile() && dest.toLowerCase().endsWith(".md")) {
        if (!anchorsOf(dest).has(anchor.toLowerCase())) {
          findings.push({ file: rel, line, target, problem: "missing anchor" });
        }
      }
    }
  }
}

if (process.argv.includes("--json")) {
  console.log(JSON.stringify({ files: files.length, findings }, null, 2));
} else {
  for (const f of findings) console.error(`${f.file}:${f.line}: ${f.problem}: ${f.target}`);
  console.log(
    `Checked ${files.length} Markdown files: ${findings.length === 0 ? "no broken links or anchors" : `${findings.length} broken link(s)/anchor(s)`}.`,
  );
}
process.exit(findings.length === 0 ? 0 : 1);
