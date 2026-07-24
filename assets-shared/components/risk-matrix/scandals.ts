import type { Scandal, RiskEvent } from "./types";
import { ENTITIES, EXPOSURES } from "./exposures";

// scandals.ts — DERIVED from the SSOT (exposures.ts). DO NOT edit data here:
// edit exposures.ts, the single source of truth. This adapter groups EXPOSURES
// by entity and joins the ENTITIES node-metadata into the `SCANDALS` map the
// RiskMatrix component consumes. Each event's punchy `headline` becomes its
// `title`; `explanation` becomes `desc`. `TECH_ORDER` / `AGENCY_ORDER` are
// re-exported unchanged from the SSOT. `SOURCES.md` remains the citation ledger.

function buildScandals(): Record<string, Scandal> {
  const out: Record<string, Scandal> = {};
  for (const key of Object.keys(ENTITIES)) {
    out[key] = { ...ENTITIES[key], events: [] };
  }
  for (const ex of EXPOSURES) {
    const scandal = out[ex.entity];
    if (!scandal) continue; // exposures.ts guarantees ex.entity ∈ ENTITIES
    const event: RiskEvent = {
      year: ex.year,
      severity: ex.severity,
      title: ex.headline,
      desc: ex.explanation,
      sources: ex.sources,
    };
    if (ex.image) event.image = ex.image;
    scandal.events.push(event);
  }
  return out;
}

export const SCANDALS: Record<string, Scandal> = buildScandals();

export { TECH_ORDER, AGENCY_ORDER } from "./exposures";
