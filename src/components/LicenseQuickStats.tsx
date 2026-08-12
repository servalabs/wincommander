import { licensePlanLabel } from "../utils/licensePresentation";

interface Props {
  plan?: string | null;
  /** null means no expiry (perpetual). */
  daysRemaining: number | null;
  seatsUsed?: number | null;
  seatLimit?: number | null;
}

/**
 * Compact plan names for the 248px sidebar. Anything the server sends that is
 * not listed here (internal/QA plans, future SKUs) falls through to
 * `licensePlanLabel` and is ellipsised by CSS with the full value in `title` —
 * the row must never widen to fit a plan name.
 */
const COMPACT_PLAN_LABELS: Record<string, string> = {
  pro_lifetime: "Pro LFT",
  pro_membership: "Pro MEM",
  investigator: "Investig",
  fleet: "Fleet",
  trial: "Trial",
  paid: "Pro",
  pro: "Pro",
  all: "Pro",
};

function compactPlan(plan?: string | null): string {
  if (!plan) return licensePlanLabel(plan);
  return COMPACT_PLAN_LABELS[plan.toLowerCase()] ?? licensePlanLabel(plan);
}

/**
 * Day counts run to four digits on long-dated licences (an internal key showed
 * 3609d), which no 70px column can hold. Anything past ~3 months reads better —
 * and far narrower — on a coarser unit.
 */
function compactRemaining(days: number | null): string {
  if (days === null) return "∞";
  if (days < 100) return `${days}d`;
  if (days < 365) return `${Math.round(days / 30)}mo`;
  return `${(days / 365).toFixed(1)}y`;
}

function remainingTitle(days: number | null): string {
  if (days === null) return "No expiry date — this licence does not lapse";
  return `${days} day${days === 1 ? "" : "s"} remaining`;
}

/** The three licence facts as labelled cells: each value is truncated with the
 *  full text in `title`, so no value can bleed into its neighbour. */
export default function LicenseQuickStats({ plan, daysRemaining, seatsUsed, seatLimit }: Props) {
  const seats = `${seatsUsed ?? "?"}/${seatLimit ?? 3}`;

  return (
    <div className="license-meta">
      <div className="license-stat">
        <span className="license-stat-label">Plan</span>
        <span className="license-stat-value" title={licensePlanLabel(plan)}>{compactPlan(plan)}</span>
      </div>
      <div className="license-stat">
        <span className="license-stat-label">Expiry</span>
        <span className="license-stat-value" title={remainingTitle(daysRemaining)}>{compactRemaining(daysRemaining)}</span>
      </div>
      <div className="license-stat">
        <span className="license-stat-label">Devices</span>
        <span className="license-stat-value" title={`${seatsUsed ?? "unknown"} of ${seatLimit ?? 3} device slots in use`}>{seats}</span>
      </div>
    </div>
  );
}
