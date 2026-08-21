// tools/check-tier-drift.ts
//
// ═══════════════════════════════════════════════════════════════════════
// CI INVARIANT — TS↔Rust tier-drift check
// ═══════════════════════════════════════════════════════════════════════
//
// Tier classification is hand-maintained in TWO places that must agree:
//   - src/registry/*.toggles.ts   (ToggleDef.tier + enableCmd/disableCmd)
//   - src-tauri/commander-free/src/backend.rs (SENSITIVE_COMMANDS tilde-split
//     const + the get_command_tier() fallback match arms — the runtime gate
//     that actually decides whether a PowerShell command executes)
//
// check-tier-invariants.ts only validates TS-internal consistency; it never
// reads backend.rs. That gap is how BitLocker TPM+PIN drifted (TS said one
// tier, the Rust runtime gate enforced another) — see project memory
// "WinCommander vault→Pro-only shift" / "tier-drift" fix, 2026-07-10.
//
// This script parses backend.rs as TEXT (no cargo/rustc needed — cheap
// enough to run in the same ubuntu-latest `lint:tiers` job) and reconstructs
// the (command_name -> tier) map exactly as get_command_tier() resolves it
// at runtime, then cross-references every toggle's enableCmd/disableCmd
// against it.
//
// Only the toggle's enableCmd is compared strictly against toggle.tier: by
// documented design (see backend.rs comments on Enable-UAC/Enable-*Bypass
// etc.) the "re-harden" direction (disableCmd) is often intentionally FREE
// even when the toggle itself is tier:"paid" — undoing a paid-gated risky
// change is never paid-gated. So disableCmd is checked one-directionally:
// it must never resolve MORE restrictive (paid) than the toggle's own tier
// would allow for a free user, i.e. if toggle.tier === "free", disableCmd
// must not resolve to "paid" in backend.rs (that would silently strand free
// users unable to turn a free-tier setting back off).
//
// Run via: bun run tools/check-tier-drift.ts
// Wired into CI by `lint:tiers` (.github/workflows/invariants.yml,
// `tier-invariants` job — already a hard/blocking gate, no cargo needed).

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ALL_TOGGLES } from "../src/registry";

type Tier = "free" | "paid";

const BACKEND_RS = join(
  import.meta.dir,
  "..",
  "src-tauri",
  "commander-free",
  "src",
  "backend.rs",
);

// ── Strip Rust line comments (but not inside string literals — command
// names never contain "//", so a straightforward line-based strip is safe) ──
function stripLineComments(src: string): string {
  return src
    .split("\n")
    .map((line) => {
      const idx = line.indexOf("//");
      return idx === -1 ? line : line.slice(0, idx);
    })
    .join("\n");
}

// ── Extract a brace/bracket-balanced block starting at the first `open`
// after `fromIndex`, matching `open`/`close` characters. Returns the
// interior text (excluding the outer open/close) and the index just past
// the closing delimiter. ──
function extractBalanced(
  src: string,
  fromIndex: number,
  open: string,
  close: string,
): { body: string; endIndex: number } {
  const start = src.indexOf(open, fromIndex);
  if (start === -1) {
    throw new Error(`check-tier-drift: could not find '${open}' after index ${fromIndex}`);
  }
  let depth = 0;
  let i = start;
  for (; i < src.length; i++) {
    if (src[i] === open) depth++;
    else if (src[i] === close) {
      depth--;
      if (depth === 0) {
        return { body: src.slice(start + 1, i), endIndex: i + 1 };
      }
    }
  }
  throw new Error(`check-tier-drift: unbalanced '${open}'/'${close}' starting at ${start}`);
}

// ── 1. SENSITIVE_COMMANDS: tilde-split `parts` arrays ────────────────────
// Reconstructs names via the exact join_parts() logic in
// wincmd-shared/src/command_strings.rs: strip every '~', concatenate.
function joinParts(parts: string[]): string {
  return parts.map((p) => p.replace(/~/g, "")).join("");
}

function parseSensitiveCommands(rawSrc: string): Map<string, Tier> {
  const src = stripLineComments(rawSrc);
  const marker = "const SENSITIVE_COMMANDS: &[SensitiveCommand] = &[";
  const markerIdx = src.indexOf(marker);
  if (markerIdx === -1) {
    throw new Error("check-tier-drift: SENSITIVE_COMMANDS const not found in backend.rs");
  }
  const { body } = extractBalanced(src, markerIdx + marker.length - 1, "[", "]");

  const map = new Map<string, Tier>();
  // Each entry looks like:
  //   SensitiveCommand {
  //       parts: &["Disable~-", "UAC~"],
  //       frontend_module: ...,
  //       backend_module: ...,
  //       tier: CommandTier::Paid,
  //   },
  const entryRe =
    /parts:\s*&\[([^\]]*)\][\s\S]*?tier:\s*CommandTier::(Free|Paid)/g;
  let m: RegExpExecArray | null;
  while ((m = entryRe.exec(body)) !== null) {
    const partsRaw = m[1];
    const parts = Array.from(partsRaw.matchAll(/"([^"]*)"/g)).map((mm) => mm[1]);
    if (parts.length === 0) continue;
    const name = joinParts(parts);
    const tier: Tier = m[2] === "Paid" ? "paid" : "free";
    map.set(name, tier);
  }
  return map;
}

// ── 2. register_p2_commands / register_p3_commands: plain CommandEntry
// literals (command: "Name", ..., tier: CommandTier::X). These are
// registered into the runtime registry, which get_command_tier() consults
// BEFORE the SENSITIVE_COMMANDS/fallback paths, so they take precedence. ──
function parseRegisteredCommandEntries(rawSrc: string): Map<string, Tier> {
  const src = stripLineComments(rawSrc);
  const map = new Map<string, Tier>();
  for (const fnName of ["register_p2_commands", "register_p3_commands"]) {
    const fnMarker = `pub fn ${fnName}(`;
    const fnIdx = src.indexOf(fnMarker);
    if (fnIdx === -1) continue; // optional — don't fail if a function is renamed/removed
    const { body } = extractBalanced(src, fnIdx, "{", "}");
    const entryRe =
      /command:\s*"([^"]+)"[\s\S]*?tier:\s*CommandTier::(Free|Paid)/g;
    let m: RegExpExecArray | null;
    while ((m = entryRe.exec(body)) !== null) {
      map.set(m[1], m[2] === "Paid" ? "paid" : "free");
    }
  }
  return map;
}

// ── 3. get_command_tier() fallback match arms: `"Cmd1" | "Cmd2" => "paid",`
// (possibly spanning many lines with comments already stripped). ──
function parseFallbackMatchArms(rawSrc: string): Map<string, Tier> {
  const src = stripLineComments(rawSrc);
  const fnMarker = "fn get_command_tier(command: &str) -> &'static str {";
  const fnIdx = src.indexOf(fnMarker);
  if (fnIdx === -1) {
    throw new Error("check-tier-drift: get_command_tier() not found in backend.rs");
  }
  const { body: fnBody } = extractBalanced(src, fnIdx, "{", "}");

  const matchMarker = "match command {";
  const matchIdx = fnBody.indexOf(matchMarker);
  if (matchIdx === -1) {
    throw new Error("check-tier-drift: 'match command {' not found in get_command_tier()");
  }
  const { body: matchBody } = extractBalanced(
    fnBody,
    matchIdx + matchMarker.length - 1,
    "{",
    "}",
  );

  const map = new Map<string, Tier>();
  // Walk arm-by-arm: each arm ends at `=> "free"` or `=> "paid"`. The
  // pattern (list of "Cmd" | "Cmd2" | ...) is everything since the end of
  // the previous arm.
  const armEndRe = /=>\s*"(free|paid)"/g;
  let lastEnd = 0;
  let m: RegExpExecArray | null;
  while ((m = armEndRe.exec(matchBody)) !== null) {
    const patternText = matchBody.slice(lastEnd, m.index);
    const tier = m[1] as Tier;
    for (const strMatch of patternText.matchAll(/"([^"]+)"/g)) {
      map.set(strMatch[1], tier);
    }
    lastEnd = armEndRe.lastIndex;
  }
  return map;
}

interface BackendResolution {
  tier: Tier;
  source: "registry" | "sensitive" | "fallback" | "default-free";
}

// Some enableCmd/disableCmd values name direct #[tauri::command] handlers
// (e.g. Argus: argus_app_usage_start) that gate their own paid entitlement
// inline via license::require_paid() and never go through
// run_backend_script()/get_command_tier() at all — they don't appear
// anywhere in backend.rs. Applying get_command_tier()'s "unknown => free"
// default to those would be a false positive (they're not "unknown to the
// PS-dispatch gate", they're simply not PS-dispatched). So: only apply the
// strict tier check to commands that appear at least once, in some form, in
// backend.rs — that's the actual scope of what this file's tier gate can
// drift on.
function collectKnownStrings(rawSrc: string): Set<string> {
  const src = stripLineComments(rawSrc);
  const known = new Set<string>();
  for (const m of src.matchAll(/"([^"\\]*)"/g)) {
    known.add(m[1]);
  }
  return known;
}

function buildBackendResolver(
  rawSrc: string,
): (command: string) => BackendResolution | null {
  const registryMap = parseRegisteredCommandEntries(rawSrc);
  const sensitiveMap = parseSensitiveCommands(rawSrc);
  const fallbackMap = parseFallbackMatchArms(rawSrc);
  const knownStrings = collectKnownStrings(rawSrc);

  return (command: string): BackendResolution | null => {
    if (registryMap.has(command)) {
      return { tier: registryMap.get(command)!, source: "registry" };
    }
    if (sensitiveMap.has(command)) {
      return { tier: sensitiveMap.get(command)!, source: "sensitive" };
    }
    if (fallbackMap.has(command)) {
      return { tier: fallbackMap.get(command)!, source: "fallback" };
    }
    if (!knownStrings.has(command)) {
      // Not part of the PS-dispatch surface at all (e.g. a direct
      // #[tauri::command] gated by inline require_paid()) — out of scope.
      return null;
    }
    // get_command_tier()'s documented behavior: unknown-but-dispatched
    // commands default free.
    return { tier: "free", source: "default-free" };
  };
}

function main(): void {
  let rawSrc: string;
  try {
    rawSrc = readFileSync(BACKEND_RS, "utf8");
  } catch (err) {
    console.error(`[tier-drift] FAIL — could not read ${BACKEND_RS}: ${String(err)}`);
    process.exit(1);
  }

  let resolve: (command: string) => BackendResolution | null;
  try {
    resolve = buildBackendResolver(rawSrc!);
  } catch (err) {
    // Parse failure in backend.rs's structure — treat as a hard failure
    // rather than silently skipping the check (a parse break is exactly
    // the kind of drift-enabling gap this tool exists to prevent).
    console.error(`[tier-drift] FAIL — could not parse backend.rs: ${String(err)}`);
    process.exit(1);
  }

  const violations: string[] = [];
  let checkedPairs = 0;
  let skippedOutOfScope = 0;

  for (const toggle of ALL_TOGGLES) {
    const cmds: Array<{ field: "enableCmd" | "disableCmd"; cmd: string | undefined }> = [
      { field: "enableCmd", cmd: (toggle as { enableCmd?: string }).enableCmd },
      { field: "disableCmd", cmd: (toggle as { disableCmd?: string }).disableCmd },
    ];

    for (const { field, cmd } of cmds) {
      if (!cmd) continue;
      const resolution = resolve(cmd);
      if (resolution === null) {
        skippedOutOfScope++;
        continue; // not PS-dispatched via backend.rs; different gating mechanism
      }
      checkedPairs++;

      if (field === "enableCmd") {
        // The gating direction: backend tier must match the declared toggle tier
        // exactly. This is the direct BitLocker-class check.
        if (resolution.tier !== toggle.tier) {
          violations.push(
            `${toggle.id}.enableCmd "${cmd}": toggle declares tier="${toggle.tier}" but backend.rs resolves tier="${resolution.tier}" (via ${resolution.source})`,
          );
        }
      } else {
        // The re-harden direction: intentionally allowed to be free even when
        // the toggle is paid (see backend.rs comments). Only flag the direction
        // that actually strands users: a free-tier toggle whose disableCmd
        // secretly requires paid.
        if (toggle.tier === "free" && resolution.tier === "paid") {
          violations.push(
            `${toggle.id}.disableCmd "${cmd}": toggle declares tier="free" but backend.rs resolves tier="paid" (via ${resolution.source}) — free users could not turn this back off`,
          );
        }
      }
    }
  }

  if (violations.length === 0) {
    console.log(
      `[tier-drift] OK — ${checkedPairs} enableCmd/disableCmd references across ${ALL_TOGGLES.length} toggles agree with backend.rs (${skippedOutOfScope} skipped: not PS-dispatched via backend.rs).`,
    );
    process.exit(0);
  }

  console.error(`[tier-drift] FAIL — ${violations.length} TS↔Rust tier mismatch(es):`);
  for (const v of violations) {
    console.error(`  - ${v}`);
  }
  process.exit(1);
}

main();
