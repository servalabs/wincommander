import { useEffect, useId, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-shell";
import { Button } from "@/components/ui/button";
import type { AppLicenseStatus } from "../hooks/useBackend";
import { type PurchaseSku, usePurchase, usePurchaseCatalog } from "../hooks/usePurchase";
import LicenseOfferCard from "./LicenseOfferCard";
import LicenseOfferPoints from "./LicenseOfferPoints";
import LicensePendingPurchase from "./LicensePendingPurchase";
import LicensePurchaseTerms from "./LicensePurchaseTerms";

interface Props {
  licenseStatus: AppLicenseStatus | null;
  onActivated: () => void;
  onStartTrial: () => void;
  isLicenseBusy: boolean;
}

const validEmail = (value: string) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value.trim());

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
  // Phone is optional, so it stays out of the pay path until asked for. One-way
  // reveal: hiding it again could submit a value the buyer can no longer see.
  const [isPhoneShown, setIsPhoneShown] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const purchase = usePurchase(onActivated);
  const catalog = usePurchaseCatalog();
  const ringId = useId();
  const emailId = useId();
  const phoneId = useId();
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

  useEffect(() => {
    if (selected?.sku !== "fleet" || selected.minSeats == null || selected.maxSeats == null) return;
    setFleetSeats((value) => Math.min(selected.maxSeats!, Math.max(selected.minSeats!, value)));
  }, [selected?.sku, selected?.minSeats, selected?.maxSeats]);

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
  const activePurchaseOffer = purchase.pending
    ? visibleOffers?.find((offer) => offer.sku === purchase.pending?.sku) ?? null
    : selected;

  return (
    <div className="license-gate-buy">
      {licenseStatus?.trial_available && (
        <div className="license-gate-trial">
          <div className="license-gate-trial-left">
            <div>
              <strong>16-Day Free Trial</strong>
              <div className="license-gate-trial-sub">Pro features · No card · Once per Windows device</div>
            </div>
          </div>
          <button type="button" className="license-gate-trial-btn" disabled={isLicenseBusy} onClick={onStartTrial}>
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
          <button type="button" className="license-gate-btn-retry" onClick={() => void catalog.refresh()}>Retry</button>
        </div>
      )}
      {!purchase.pending && visibleOffers && (
        <div className="license-gate-offers" role="radiogroup" aria-label="Choose an offer">
          {visibleOffers.map((offer) => (
            <LicenseOfferCard
              key={offer.sku}
              offer={offer}
              isSelected={sku === offer.sku}
              ringId={ringId}
              onSelect={() => setSku(offer.sku)}
            />
          ))}
        </div>
      )}

      {/* The selected plan's terms appear exactly once, here. */}
      {selected && !purchase.pending && <LicenseOfferPoints offer={selected} />}
      {selected && !purchase.pending && !selected.checkoutEligible && (
        <div className="license-gate-message">
          {selected.checkoutMessage ?? "This offer requires a reviewed written order. Contact legal@servalabs.com."}
        </div>
      )}

      {!purchase.pending && selected?.sku === "fleet" && selected.minSeats != null && selected.maxSeats != null && (
        <div className="license-gate-config-row">
          <span className="license-gate-label">Managed devices</span>
          <div className="license-gate-stepper">
            <button
              type="button"
              className="license-gate-stepper-btn"
              aria-label={`Decrease managed devices; minimum ${selected.minSeats}`}
              onClick={() => setFleetSeats((value) => Math.max(selected.minSeats!, value - 1))}
              disabled={fleetSeats <= selected.minSeats}
            >−</button>
            <span
              className="license-gate-stepper-val"
              role="status"
              aria-live="polite"
              aria-label={`${fleetSeats} managed devices selected`}
            >{fleetSeats}</span>
            <button
              type="button"
              className="license-gate-stepper-btn"
              aria-label={`Increase managed devices; maximum ${selected.maxSeats}`}
              onClick={() => setFleetSeats((value) => Math.min(selected.maxSeats!, value + 1))}
              disabled={fleetSeats >= selected.maxSeats}
            >+</button>
          </div>
          {selected.seatPricingLabel && <span className="license-gate-config-hint">{selected.seatPricingLabel}</span>}
        </div>
      )}

      {!purchase.pending && (
        <>
          <div className="license-gate-field">
            <label className="license-gate-label" htmlFor={emailId}>Email — your license key is delivered here</label>
            <input
              id={emailId}
              type="email"
              className={`license-gate-input${email && !emailOk ? " input-error" : ""}`}
              placeholder="you@example.com"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
            />
          </div>
          {isPhoneShown ? (
            <div className="license-gate-field">
              <label className="license-gate-label" htmlFor={phoneId}>Phone (optional, international format)</label>
              <input
                id={phoneId}
                type="tel"
                className="license-gate-input"
                placeholder="+919876543210"
                value={phone}
                onChange={(event) => setPhone(event.target.value.replace(/[^\d+]/g, ""))}
                autoFocus
              />
            </div>
          ) : (
            <button type="button" className="license-gate-inline-toggle" onClick={() => setIsPhoneShown(true)}>
              + Add phone (optional)
            </button>
          )}
        </>
      )}

      {(notice || purchase.error) && <div className="license-gate-message">{notice ?? purchase.error}</div>}

      <div className="license-gate-pay-row">
        <span className="license-gate-quote">
          {amount != null && currency
            ? `Checkout total: ${new Intl.NumberFormat("en-US", { style: "currency", currency }).format(amount / 100)}`
            : selected ? selected.priceLabel : "Select a current offer to continue."}
        </span>
        <LicensePurchaseTerms hasInvestigatorEntitlement={hasInvestigatorEntitlement} />
      </div>

      {purchase.pending?.purchaseId ? (
        <LicensePendingPurchase
          purchase={purchase}
          offer={activePurchaseOffer}
          hasInvestigatorEntitlement={hasInvestigatorEntitlement}
          onReopenCheckout={() => void reopenCheckout()}
        />
      ) : (
        <Button
          type="button"
          variant="primary"
          size="lg"
          className="w-full font-[family-name:var(--font-mono)] text-[12px] uppercase tracking-[1.5px]"
          disabled={!emailOk || purchase.isStarting || !selected || !selected.checkoutEligible}
          onClick={() => void startCheckout()}
        >
          {purchase.isStarting
            ? "Creating secure checkout…"
            : selected?.checkoutEligible
              ? `Continue with ${selected.name}`
              : selected
                ? "Contact ServaLabs for review"
                : "Offers unavailable"}
        </Button>
      )}
    </div>
  );
}
