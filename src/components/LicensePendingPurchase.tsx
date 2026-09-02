import LicenseOfferPoints from "./LicenseOfferPoints";
import type { CatalogOffer, usePurchase } from "../hooks/usePurchase";
import { useAppConfirm } from "./shared/AppConfirmDialog";

interface Props {
  purchase: ReturnType<typeof usePurchase>;
  /** The offer the pending checkout was created for, or the current selection. */
  offer: CatalogOffer | null;
  hasInvestigatorEntitlement: boolean;
  onReopenCheckout: () => void;
}

/** Everything after `create_purchase` succeeded: checkout resume, reconcile,
 *  the delivered license key, and the per-plan follow-up hints. */
export default function LicensePendingPurchase({
  purchase,
  offer,
  hasInvestigatorEntitlement,
  onReopenCheckout,
}: Props) {
  const confirmAction = useAppConfirm();
  const state = purchase.status?.state;

  return (
    <div className="license-gate-purchase-state">
      <div className="license-gate-purchase-state-info">
        <strong>
          {purchase.status?.activated
            ? "License activated"
            : state === "payment_failed"
              ? "Payment failed — retry the same order"
              : "Checkout ready"}
        </strong>
        <div className="license-gate-purchase-state-hint">
          {purchase.status?.activated
            ? "Your licence is active on this PC."
            : "Complete payment in your browser. WinCommander will detect it automatically; you can safely close and reopen this window."}
        </div>
        {offer ? (
          <LicenseOfferPoints offer={offer} />
        ) : (
          <div className="license-gate-purchase-state-hint">
            This checkout resumes automatically after an app restart.
          </div>
        )}
        {purchase.status?.providerStatus === "authorized" && (
          <div className="license-gate-purchase-state-hint">Razorpay has authorized the payment but has not captured it yet. Do not pay again; check again shortly.</div>
        )}
        {offer?.sku === "pro_membership" && purchase.status?.activated && (
          <div className="license-gate-purchase-state-hint">Use the same billing email when creating or signing in to your hosted Netwall account.</div>
        )}
        {hasInvestigatorEntitlement && offer?.sku === "investigator" && purchase.status?.activated && (
          <div className="license-gate-purchase-state-hint">Investigator Mode and Netwall follow the subscription term; normal Pro does not self-disable when the term ends.</div>
        )}
        {offer?.sku === "fleet" && purchase.status?.activated && (
          <div className="license-gate-purchase-state-hint">Your Fleet subscription covers {purchase.pending?.seats ?? 1} managed endpoint{purchase.pending?.seats === 1 ? "" : "s"}. Configure the customer-hosted Fleet server before enrolling endpoints.</div>
        )}
        {!purchase.status?.activated && (
          <button className="license-gate-trial-btn" disabled={purchase.isOpening} onClick={onReopenCheckout}>
            {purchase.isOpening ? "Preparing secure checkout…" : "Open secure checkout"}
          </button>
        )}
        {!purchase.status?.activated && (
          <button className="license-gate-btn-retry" onClick={() => void purchase.reconcile()}>I’ve paid — verify with Razorpay</button>
        )}
        {purchase.status?.licenseKey && (
          <>
            <div className="license-gate-license-key">{purchase.status.licenseKey}</div>
            <button className="license-gate-btn-retry" onClick={() => void navigator.clipboard.writeText(purchase.status!.licenseKey!)}>Copy key</button>
            <button className="license-gate-btn-retry" onClick={() => void purchase.resend()}>Resend email</button>
          </>
        )}
        {purchase.status?.activated && offer?.sku !== "pro_lifetime" && purchase.cancellationEffectiveAt === undefined && (
          <button
            className="license-gate-btn-retry"
            disabled={purchase.isCancelling}
            onClick={() => void (async () => {
              const accepted = await confirmAction({
                title: "Cancel subscription renewal?",
                description: "Renewal will stop at the end of the current paid billing cycle. This does not refund the current cycle.",
                confirmLabel: "Cancel renewal",
              });
              if (accepted) await purchase.cancelSubscription();
            })()}
          >
            {purchase.isCancelling ? "Cancelling renewal..." : "Cancel subscription renewal"}
          </button>
        )}
        {purchase.cancellationEffectiveAt !== undefined && (
          <div className="license-gate-purchase-state-hint">
            Renewal is cancelled. Access continues {purchase.cancellationEffectiveAt
              ? `until ${new Date(purchase.cancellationEffectiveAt * 1000).toLocaleDateString()}`
              : "through the current paid billing cycle"}.
          </div>
        )}
        <button
          className="license-gate-btn-retry"
          title="Clears this pending checkout from this PC so you can choose another offer. It does not cancel or refund a completed payment."
          onClick={() => void purchase.forget()}
        >
          Start over
        </button>
      </div>
    </div>
  );
}
