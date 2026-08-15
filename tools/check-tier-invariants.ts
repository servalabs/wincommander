// tools/check-tier-invariants.ts
//
// ═══════════════════════════════════════════════════════════════════════
// CI INVARIANT — Tier + risk-axis correctness
// ═══════════════════════════════════════════════════════════════════════
//
// Loads the toggle + feature registries and asserts every entry obeys
// the four invariants in ref/architecture.md (Open-Core Architecture — CI invariants):
//
//   1. tier in {"free", "paid"}                         (TS already enforces)
//   2. irreversible: true   ⇒ needsAdmin: true
//   3. irreversible: true   ⇒ tier === "paid"
//   4. defenderFlagged: true ⇒ tier === "paid"
//   5. reducesSecurity: true ⇒ tier === "paid"
//      (except REDUCES_SECURITY_FREE_EXEMPT — registry-only, see below)
//   6. tier === "free"      ⇒ !defenderFlagged   (corollary of 4)
//
// Exit 0 = all pass. Exit 1 = at least one violation, with a printed
// summary listing every offending entry. Run via:
//
//   bun run tools/check-tier-invariants.ts
//
// Wired into CI by .github/workflows/invariants.yml.

import { ALL_TOGGLES } from "../src/registry";
import { BLOCKLISTS, FEATURES } from "../src/registry/features";
import type { ToggleDef, Tier } from "../src/types/toggles";

interface RegistryEntry {
  registry: string;
  id: string;
  tier: Tier;
  needsAdmin: boolean;
  irreversible: boolean;
  reducesSecurity: boolean;
  defenderFlagged: boolean;
}

const VALID_TIERS: ReadonlySet<Tier> = new Set<Tier>(["free", "paid"]);

// ── Invariant 5 exemptions ─────────────────────────────────────────────
//
// reducesSecurity ⇒ paid exists because weakening a security feature is
// normally done by sidecar code that AV flags (Defender disablement, VBS
// teardown, OOBE bypass). It is a proxy for "this needs the Pro sidecar",
// not a rule that every security-reducing setting must be sold.
//
// These entries are plain HKLM registry DWORD writes: nothing to AV-flag,
// nothing irreversible, and the reducesSecurity flag still drives the
// warning dialog in the UI. They are Windows Server administration basics
// and shipping them behind the paywall would be arbitrary. Keep this list
// SHORT and registry-only — anything touching Defender, VBS, SmartScreen,
// or the OOBE flow belongs in the sidecar and must not be added here.
const REDUCES_SECURITY_FREE_EXEMPT: ReadonlySet<string> = new Set([
  "toggles/serverCtrlAltDel", // DisableCAD — drops the logon SAS requirement
  "toggles/serverIeEsc",      // IE ESC Active Setup IsInstalled=0
]);

function toEntry(registry: string, t: ToggleDef | { id: string; tier: Tier; needsAdmin: boolean; irreversible: boolean; reducesSecurity: boolean; defenderFlagged: boolean }): RegistryEntry {
  return {
    registry,
    id: t.id,
    tier: t.tier,
    needsAdmin: t.needsAdmin,
    irreversible: t.irreversible,
    reducesSecurity: t.reducesSecurity,
    defenderFlagged: t.defenderFlagged,
  };
}

function checkAll(entries: RegistryEntry[]): string[] {
  const violations: string[] = [];

  for (const e of entries) {
    const where = `${e.registry}/${e.id}`;

    if (!VALID_TIERS.has(e.tier)) {
      violations.push(`${where}: tier="${e.tier}" must be "free" or "paid"`);
    }

    if (e.irreversible && !e.needsAdmin) {
      violations.push(`${where}: irreversible=true requires needsAdmin=true`);
    }

    if (e.irreversible && e.tier !== "paid") {
      violations.push(`${where}: irreversible=true requires tier="paid", got "${e.tier}"`);
    }

    if (e.defenderFlagged && e.tier !== "paid") {
      violations.push(`${where}: defenderFlagged=true requires tier="paid", got "${e.tier}"`);
    }

    if (e.reducesSecurity && e.tier !== "paid" && !REDUCES_SECURITY_FREE_EXEMPT.has(where)) {
      violations.push(`${where}: reducesSecurity=true requires tier="paid", got "${e.tier}"`);
    }

    if (e.tier === "free" && e.defenderFlagged) {
      // Corollary of the above; surfaces as a clearer message in the
      // common case ("you put a Defender-flagged feature in the Free
      // binary — it's going to AV-flag the installer").
      violations.push(
        `${where}: tier="free" must not have defenderFlagged=true (would taint the AV-clean Free binary)`,
      );
    }
  }

  return violations;
}

function main(): void {
  const entries: RegistryEntry[] = [
    ...ALL_TOGGLES.map((t) => toEntry("toggles", t)),
    ...BLOCKLISTS.map((b) => toEntry("blocklists", b)),
    ...FEATURES.map((f) => toEntry("features", f)),
  ];

  const violations = checkAll(entries);

  if (violations.length === 0) {
    console.log(
      `[invariants] OK — ${entries.length} entries (${ALL_TOGGLES.length} toggles, ${BLOCKLISTS.length} blocklists, ${FEATURES.length} features) pass all 6 tier+risk invariants.`,
    );
    process.exit(0);
  }

  console.error(
    `[invariants] FAIL — ${violations.length} violation(s) across ${entries.length} entries:`,
  );
  for (const v of violations) {
    console.error(`  - ${v}`);
  }
  console.error(
    "\nSee ref/architecture.md (Open-Core Architecture — CI invariants) for the full invariant list.",
  );
  process.exit(1);
}

main();
