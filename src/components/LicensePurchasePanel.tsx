import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-shell";
import type { AppLicenseStatus } from "../hooks/useBackend";
import { type PurchaseSku, usePurchase, usePurchaseCatalog } from "../hooks/usePurchase";

interface Props {
  licenseStatus: AppLicenseStatus | null;
  onActivated: () => void;
  onStartTrial: () => void;
  isLicenseBusy: boolean;
}

const validEmail = (value: string) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value.trim());

const OFFER_BADGES: Record<PurchaseSku, string[]> = {
  pro_lifetime: ["Pay once", "Lifetime updates"],
  pro_membership: ["Subscription", "Netwall included"],
  investigator: ["Subscription", "Netwall included"],
  fleet: ["Subscription", "Netwall included"],
};

const OFFER_PROMISES: Record<PurchaseSku, string> = {
  pro_lifetime: "Normal Pro on 3 transferable active devices. Pay once; Pro updates never expire.",
  pro_membership: "Pro updates and hosted Netwall on 3 transferable active devices. If you stop, your last eligible normal-Pro build keeps working.",
  investigator: "Investigator Mode, Pro updates, and hosted Netwall on 3 transferable active devices. If you stop, cases stay readable and normal Pro keeps working.",
  fleet: "One managed endpoint per seat. Fleet and Netwall are included. After term/grace, management becomes read-only without deleting policies or data.",
};

export default function LicensePurchasePanel({
  licenseStatus,
  onActivated,
  onStartTrial,
  isLicenseBusy,
}: Props) {
  const [sku, setSku] = useState<PurchaseSku | null>(null);
  const [fleetSeats, setFleetSeats] = useState(1);
  const [email, setEmail] = useState("");
  const [phone, setPhone] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const purchase = usePurchase(onActivated);
  const catalog = usePurchaseCatalog();
  const emailOk = validEmail(email);
  const hasInvestigatorEntitlement =
    licenseStatus?.valid === true && (licenseStatus.features ?? []).includes("advanced");
  const visibleOffers = useMemo(
    () => catalog.offers?.filter(
      (offer) => offer.sku !== "investigator" || hasInvestigatorEntitlement,
    ),
    [catalog.offers, hasInvestigatorEntitlement],
  );
  const selected = visibleOffers?.find((offer) => offer.sku === sku) ?? null;

  useEffect(() => {
    if (!visibleOffers?.length) return;
    if (!sku || !visibleOffers.some((offer) => offer.sku === sku)) {
      setSku(visibleOffers[0].sku);
    }
  }, [visibleOffers, sku]);

  const startCheckout = async () => {
    setNotice(null);
    try {
      const pending = await purchase.start({
        sku: selected!.sku,
        ...(selected!.sku === "fleet" ? { seats: fleetSeats } : {}),
        email: email.trim(),
        ...(phone.trim() ? { phone: phone.trim() } : {}),
      });
      if (!pending.checkoutUrl) throw new Error("Secure checkout URL was not returned.");
      await open(pending.checkoutUrl);
    } catch (error) {
      setNotice(String(error));
    }
  };

  const reopenCheckout = async () => {
    setNotice(null);
    try {
      const pending = await purchase.resume();
      if (!pending.checkoutUrl) throw new Error("Secure checkout URL was not returned.");
      await open(pending.checkoutUrl);
    } catch (error) {
      setNotice(String(error));
    }
  };

  const amount = purchase.status?.amount ?? purchase.pending?.amount;
  const currency = purchase.status?.currency ?? purchase.pending?.currency;
  const state = purchase.status?.state;
  const activePurchaseOffer = purchase.pending
    ? visibleOffers?.find((offer) => offer.sku === purchase.pending?.sku) ?? null
    : selected;

  return (
    <div className="license-gate-buy">
      <div className="license-gate-free">
        <strong>Free stays free</strong>
        <span>The core WinCommander app works without a licence or payment. Upgrade only for Pro capabilities, services, or managed Fleet endpoints.</span>
      </div>

      {licenseStatus?.trial_available && (
        <div className="license-gate-trial">
          <div className="license-gate-trial-left">
            <div>
              <strong>16-Day Free Trial</strong>
              <div className="license-gate-trial-sub">Pro features · No card · Once per Windows device</div>
            </div>
          </div>
          <button className="license-gate-trial-btn" disabled={isLicenseBusy} onClick={onStartTrial}>
            {isLicenseBusy ? "Starting…" : "Start Free Trial"}
          </button>
        </div>
      )}

      {!purchase.pending && catalog.isLoading && <div className="license-gate-server-quote">Loading current offers…</div>}
      {!purchase.pending && catalog.error && (
        <div className="license-gate-message">
          Current offers are unavailable. Reconnect to view current pricing.
          <span className="license-gate-error-detail">
            {catalog.error.replace(/^Error:\s*/i, "")}
          </span>
          <button className="license-gate-btn-retry" onClick={() => void catalog.refresh()}>Retry</button>
        </div>
      )}
      {!purchase.pending && visibleOffers && (
        <div className="license-gate-offers" role="radiogroup" aria-label="Choose an offer">
        {visibleOffers.map((offer) => (
          <button
            key={offer.sku}
            type="button"
            className={`license-gate-offer${sku === offer.sku ? " active" : ""}`}
            onClick={() => setSku(offer.sku)}
            role="radio"
            aria-checked={sku === offer.sku}
          >
            <span className="license-gate-offer-heading">
              <strong>{offer.name}</strong>
              <span>{offer.priceLabel}</span>
            </span>
            <span className="license-gate-offer-badges">
              {OFFER_BADGES[offer.sku].map((badge) => <span key={badge}>{badge}</span>)}
            </span>
            <span className="license-gate-offer-detail">{offer.detail}</span>
            <span className="license-gate-offer-device">{offer.deviceRule}</span>
          </button>
        ))}
        </div>
      )}

      {!purchase.pending && selected?.sku === "fleet" && selected.minSeats != null && selected.maxSeats != null && (
        <div className="license-gate-config-row">
          <span className="license-gate-label">Managed devices</span>
          <div className="license-gate-stepper">
            <button className="license-gate-stepper-btn" onClick={() => setFleetSeats((value) => Math.max(selected.minSeats!, value - 1))} disabled={fleetSeats <= selected.minSeats}>−</button>
            <span className="license-gate-stepper-val">{fleetSeats}</span>
            <button className="license-gate-stepper-btn" onClick={() => setFleetSeats((value) => Math.min(selected.maxSeats!, value + 1))} disabled={fleetSeats >= selected.maxSeats}>+</button>
          </div>
          {selected.seatPricingLabel && <span className="license-gate-config-hint">{selected.seatPricingLabel}</span>}
        </div>
      )}

      {selected && !purchase.pending && (
        <div className="license-gate-plan-promise">{OFFER_PROMISES[selected.sku]}</div>
      )}

      <div className="license-gate-server-quote">
        {amount != null && currency
          ? `Checkout total: ${new Intl.NumberFormat("en-US", { style: "currency", currency }).format(amount / 100)}`
          : selected ? "The secure checkout service confirms the current price before payment." : "Select a current offer to continue."}
      </div>

      {!purchase.pending && (
        <>
          <div className="license-gate-field">
            <label className="license-gate-label">Email — your license key is delivered here</label>
            <input
              type="email"
              className={`license-gate-input${email && !emailOk ? " input-error" : ""}`}
              placeholder="you@example.com"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
            />
          </div>
          <div className="license-gate-field">
            <label className="license-gate-label">Phone (optional, international format)</label>
            <input
              type="tel"
              className="license-gate-input"
              placeholder="+919876543210"
              value={phone}
              onChange={(event) => setPhone(event.target.value.replace(/[^\d+]/g, ""))}
            />
          </div>
        </>
      )}

      {(notice || purchase.error) && <div className="license-gate-message">{notice ?? purchase.error}</div>}

      {purchase.pending?.purchaseId ? (
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
            <div className="license-gate-purchase-state-hint">
              {activePurchaseOffer ? OFFER_PROMISES[activePurchaseOffer.sku] : "This checkout resumes automatically after an app restart."}
            </div>
            {purchase.status?.providerStatus === "authorized" && (
              <div className="license-gate-purchase-state-hint">Razorpay has authorized the payment but has not captured it yet. Do not pay again; check again shortly.</div>
            )}
            {activePurchaseOffer?.sku === "pro_membership" && purchase.status?.activated && (
              <div className="license-gate-purchase-state-hint">Use the same billing email when creating or signing in to your hosted Netwall account.</div>
            )}
            {hasInvestigatorEntitlement && activePurchaseOffer?.sku === "investigator" && purchase.status?.activated && (
              <div className="license-gate-purchase-state-hint">Investigator Mode and Netwall follow the subscription term; normal Pro does not self-disable when the term ends.</div>
            )}
            {activePurchaseOffer?.sku === "fleet" && purchase.status?.activated && (
              <div className="license-gate-purchase-state-hint">Your Fleet subscription covers {purchase.pending.seats ?? 1} managed endpoint{purchase.pending.seats === 1 ? "" : "s"}. Configure the customer-hosted Fleet server before enrolling endpoints.</div>
            )}
            {!purchase.status?.activated && (
              <button className="license-gate-trial-btn" disabled={purchase.isOpening} onClick={() => void reopenCheckout()}>
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
            <button
              className="license-gate-btn-retry"
              title="Clears this pending checkout from this PC so you can choose another offer. It does not cancel or refund a completed payment."
              onClick={() => void purchase.forget()}
            >
              Start over
            </button>
          </div>
        </div>
      ) : (
        <button type="button" className="license-gate-trial-btn license-gate-checkout-btn" disabled={!emailOk || purchase.isStarting || !selected || !selected.checkoutEligible} onClick={() => void startCheckout()}>
          {purchase.isStarting ? "Creating secure checkout…" : selected ? `Continue with ${selected.name}` : "Offers unavailable"}
        </button>
      )}

      <div className="license-gate-buy-note">
        Checkout opens in your normal browser. WinCommander never handles payment credentials. Pro Lifetime keeps normal Pro usable permanently; memberships retain the last eligible normal Pro build if they end. Netwall is a hosted, term-based service.
        {hasInvestigatorEntitlement ? " Investigator is available only while its explicit entitlement is active." : ""}
      </div>
    </div>
  );
}
