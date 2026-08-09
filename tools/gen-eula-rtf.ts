// SPDX-License-Identifier: AGPL-3.0-or-later
// Generate the NSIS installer licence page (LICENSE.rtf) from the EULA SSOT.
//
// The installer shows this RTF as its licence page (`tauri.conf.json` ->
// `bundle.licenseFile`), so whatever is in it is what the customer actually
// accepts at install time. The EULA itself lives in the `assets` submodule and
// is rendered on servalabs.com from the same markdown. Before this generator
// existed the RTF was hand-maintained and drifted: it sat at EULA v1.4 while
// the published EULA had moved to v1.5, which meant software-only customers
// were accepting a version predating the entire reseller model (no "Reseller
// Sale", "Delivery", or "Entitlement" definitions, and superseded arbitration
// and liability-cap wording).
//
// Run `bun run gen:eula-rtf` after any EULA change; `--check` fails the build
// when the two have diverged, which is the part that stops the drift recurring.
//
// The output deliberately reproduces the style vocabulary of the original
// hand-built RTF (Arial, 10pt body, the five paragraph shapes below) so the
// installer page looks unchanged apart from the content.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { relative, resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const EULA = resolve(ROOT, "assets/legal/EULA.md");
const OUT = resolve(ROOT, "src-tauri/commander-free/nsis/LICENSE.rtf");

// The five paragraph shapes the original RTF used, kept verbatim.
const P = {
  title: String.raw`\pard\qc\sb240\sa120\b\fs28 `, // `# `   — centred, 14pt
  h2: String.raw`\pard\sb200\sa80\b\fs22 `, //         `## `  — bold, 11pt
  h3: String.raw`\pard\sb140\sa60\b `, //              `### ` — bold, 10pt
  body: String.raw`\pard\sb60\sa60 `,
  bullet: String.raw`\pard\fi-240\li480\sb40\sa40\bullet\tab `,
} as const;

function normalizePath(path: string): string {
  return relative(ROOT, path).replaceAll("\\", "/");
}

// RTF is byte-oriented and only safely carries ASCII; anything else has to go
// out as a \uN? escape with an ASCII fallback character after it. The EULA uses
// em/en dashes, curly quotes, the rupee sign and a middot, so this is not
// optional — an unescaped byte renders as mojibake in the installer.
function escapeRtf(text: string): string {
  let out = "";
  for (const char of text) {
    const code = char.codePointAt(0)!;
    if (char === "\\") out += String.raw`\\`;
    else if (char === "{") out += String.raw`\{`;
    else if (char === "}") out += String.raw`\}`;
    else if (code < 128) out += char;
    else {
      // Signed 16-bit; RTF readers want negative values above 32767.
      const signed = code > 32767 ? code - 65536 : code;
      out += `\\u${signed}?`;
    }
  }
  return out;
}

// Inline markdown -> RTF character formatting. Order matters: bold (`**`) must
// be consumed before italic (`*`) or `**x**` parses as an empty italic run.
//
// Emphasis is emitted as a brace group — `{\b text}` — rather than the
// `\b text\b0 ` toggle pair the hand-built RTF used. The toggle form has a trap:
// the space after `\b0` is the control-word delimiter and is swallowed, so
// `**lawful** data` came out as "lawfuldata". A group needs no delimiter because
// `}` ends it, so the following space survives. Groups also self-close, which
// removes the risk of an unbalanced `\b` bolding the rest of the page.
function inline(md: string): string {
  const tokens: string[] = [];
  let rest = md;

  // Links: keep the text, drop the URL. The installer page is not clickable,
  // so a bare URL would just be noise mid-sentence.
  rest = rest.replaceAll(/\[([^\]]+)\]\([^)]*\)/g, "$1");
  // Inline code carries no meaning in a licence agreement's prose.
  rest = rest.replaceAll(/`([^`]+)`/g, "$1");

  const pattern = /\*\*([^*]+)\*\*|\*([^*]+)\*/g;
  let last = 0;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(rest)) !== null) {
    tokens.push(escapeRtf(rest.slice(last, match.index)));
    if (match[1] !== undefined) tokens.push(String.raw`{\b ${escapeRtf(match[1])}}`);
    else tokens.push(String.raw`{\i ${escapeRtf(match[2]!)}}`);
    last = match.index + match[0].length;
  }
  tokens.push(escapeRtf(rest.slice(last)));

  return tokens.join("").replaceAll(/ {2,}/g, " ").trim();
}

function buildRtf(markdown: string): string {
  const lines = markdown.replaceAll("\r\n", "\n").split("\n");
  const paragraphs: string[] = [];
  let pending: string[] = [];

  // A markdown paragraph can wrap over several source lines; RTF wants it as
  // one `\par`. Buffer until a blank line or a new block-level construct.
  const flush = () => {
    if (pending.length === 0) return;
    const text = inline(pending.join(" "));
    if (text) paragraphs.push(P.body + text + String.raw`\par`);
    pending = [];
  };

  for (const raw of lines) {
    // Markdown hard break: two or more trailing spaces, or a trailing backslash.
    // Must be read before trimming, or the signal is destroyed. The signature
    // block at the foot of the EULA relies on this — without it the company
    // name, CIN, address, phone and signatory all reflow into one run-on line.
    const hardBreak = /(\s\s|\\)$/.test(raw.replace(/\r$/, ""));
    const trimmed = raw.trim().replace(/\\$/, "").trimEnd();

    if (trimmed === "") {
      flush();
      continue;
    }
    // Horizontal rules are a markdown reading aid with no RTF equivalent worth
    // emitting — the paragraph spacing already separates the sections.
    if (/^-{3,}$/.test(trimmed) || /^\*{3,}$/.test(trimmed)) {
      flush();
      continue;
    }

    const heading = trimmed.match(/^(#{1,3})\s+(.*)$/);
    if (heading) {
      flush();
      const depth = heading[1]!.length;
      const shape = depth === 1 ? P.title : depth === 2 ? P.h2 : P.h3;
      const close = depth === 1 || depth === 2 ? String.raw`\b0\fs20\par` : String.raw`\b0\par`;
      paragraphs.push(shape + inline(heading[2]!) + close);
      continue;
    }

    const bullet = trimmed.match(/^[-*+]\s+(.*)$/);
    if (bullet) {
      flush();
      paragraphs.push(P.bullet + inline(bullet[1]!) + String.raw`\par`);
      continue;
    }

    const numbered = trimmed.match(/^(\d+)\.\s+(.*)$/);
    if (numbered) {
      flush();
      // Rendered as a bullet-shaped hanging indent with the literal number, so
      // ordered and unordered lists line up on the same left edge.
      paragraphs.push(
        String.raw`\pard\fi-240\li480\sb40\sa40 ${numbered[1]}.\tab ` + inline(numbered[2]!) + String.raw`\par`,
      );
      continue;
    }

    pending.push(trimmed);
    if (hardBreak) flush();
  }
  flush();

  return [
    String.raw`{\rtf1\ansi\ansicpg1252\deff0\deflang1033`,
    String.raw`{\fonttbl{\f0\fswiss\fcharset0 Arial;}}`,
    String.raw`\widowctrl\f0\fs20`,
    ...paragraphs,
    "}",
    "",
  ].join("\n");
}

if (!existsSync(EULA)) {
  console.error(
    `EULA source not found at ${normalizePath(EULA)} — the assets submodule is probably not checked out. ` +
      "Run `git submodule update --init assets`.",
  );
  process.exit(1);
}

const markdown = readFileSync(EULA, "utf8");
const wanted = buildRtf(markdown);

// Surfaced in both modes so a version mismatch is visible in build logs.
const version = markdown.match(/\*\*Version\s+([^|*]+?)\s*\|/)?.[1]?.trim() ?? "unknown";

if (process.argv.includes("--check")) {
  const current = existsSync(OUT) ? readFileSync(OUT, "utf8").replaceAll("\r\n", "\n") : "";
  if (current !== wanted) {
    const stale = current.match(/Version\s+([\d.]+)/)?.[1] ?? "unknown";
    console.error(
      `Installer licence page is stale: ${normalizePath(OUT)} is at EULA v${stale}, source is v${version}.\n` +
        "Customers would accept the wrong version. Run `bun run gen:eula-rtf` and commit the result.",
    );
    process.exit(1);
  }
  console.log(`Installer licence page is current (EULA v${version}).`);
} else {
  writeFileSync(OUT, wanted);
  const paras = wanted.split("\n").filter((l) => l.endsWith(String.raw`\par`)).length;
  console.log(`wrote ${normalizePath(OUT)} — EULA v${version}, ${paras} paragraphs`);
}
