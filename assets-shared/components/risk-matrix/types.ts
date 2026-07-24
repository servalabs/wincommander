import type { CSSProperties, ReactNode } from "react";

export type Severity = "CRITICAL" | "HIGH" | "UNKNOWN";

/** A single citation. `label` is what the UI shows on the chip — the publisher
 *  name (e.g. "Reuters", "FTC", "EDPB") so sources read as named evidence rather
 *  than "Source 1". Chips wrap (flex-wrap, no nowrap), so long names are fine. */
export interface RiskSource {
  url: string;
  label: string;
}

export interface RiskEvent {
  year: string;
  severity: Severity;
  title: string;
  desc: string;
  sources: RiskSource[];
  /** Editorial image key, resolved via the bundled asset map (optional). */
  image?: string;
}

export interface Scandal {
  name: string;
  /** Logo key resolved via the bundled asset map (e.g. "google-logo.svg"). */
  logo: string;
  /** Node/brand accent color (any CSS color). */
  color: string;
  /** Short editorial tagline shown under the name, e.g. "The Data Harvester". */
  description: string;
  /** Placement: orbiting tech giant, or a central intel/state node. */
  category: "tech" | "agency";
  events: RiskEvent[];
}

/** Demo values shown by the bundled FingerprintMirror. All optional — the
 *  component ships sensible defaults, so hosts inject nothing to get a working
 *  panel. Country-specific hosts can pass neutral values here. */
export interface FingerprintConfig {
  tagline?: ReactNode;
  identity?: string;
  ip?: string;
  location?: string;
  isp?: string;
  timezone?: string;
}

export interface RiskMatrixProps {
  /** Extra class on the component root (host theming hook). */
  className?: string;
  /** Inline styles on the component root — the intended place to map the
   *  `--rm-*` theme variables to a host's own tokens (inline wins over the
   *  stylesheet defaults). */
  style?: CSSProperties;
  /** Small uppercase kicker above the title. */
  eyebrow?: ReactNode;
  /** Main heading. */
  title?: ReactNode;
  /** Supporting line under the heading. */
  subtitle?: ReactNode;
  /** Right-panel content shown when the YOU node is selected.
   *  Defaults to the bundled FingerprintMirror. Pass `null` to disable. */
  youPanel?: ReactNode;
  /** Config for the default FingerprintMirror (ignored when `youPanel` is set). */
  fingerprint?: FingerprintConfig;
  /** Label rendered in the YOU node (default "YOU"). */
  youLabel?: string;
  /** Optional flag image URL rendered inside the YOU node. */
  youFlagUrl?: string;
}
