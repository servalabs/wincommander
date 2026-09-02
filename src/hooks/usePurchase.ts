import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type PurchaseSku = "pro_lifetime" | "pro_membership" | "investigator" | "fleet";

export interface CatalogOffer {
  sku: PurchaseSku;
  name: string;
  priceLabel: string;
  detail: string;
  deviceRule: string;
  checkoutEligible: boolean;
  checkoutMessage?: string | null;
  minSeats?: number | null;
  maxSeats?: number | null;
  seatPricingLabel?: string | null;
}

export interface PurchaseInput {
  sku: PurchaseSku;
  seats?: number;
  email: string;
  phone?: string;
}

export interface PendingPurchase {
  purchaseId?: string;
  sku: PurchaseSku;
  seats?: number | null;
  checkoutUrl?: string;
  amount?: number;
  currency?: string;
  expiresAt?: number;
}

export interface PurchaseStatus {
  state: string;
  providerStatus?: string;
  amount?: number;
  currency?: string;
  licenseKey?: string;
  activated: boolean;
  activationError?: string;
}

export function usePurchaseCatalog() {
  const [offers, setOffers] = useState<CatalogOffer[] | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const next = await invoke<CatalogOffer[]>("get_purchase_catalog");
      setOffers(next);
    } catch (cause) {
      setOffers(null);
      setError(String(cause));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  return { offers, isLoading, error, refresh };
}

export function usePurchase(onActivated: () => void) {
  const [pending, setPending] = useState<PendingPurchase | null>(null);
  const [status, setStatus] = useState<PurchaseStatus | null>(null);
  const [isStarting, setIsStarting] = useState(false);
  const [isOpening, setIsOpening] = useState(false);
  const [isCancelling, setIsCancelling] = useState(false);
  const [cancellationEffectiveAt, setCancellationEffectiveAt] = useState<number | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const onActivatedRef = useRef(onActivated);
  onActivatedRef.current = onActivated;

  useEffect(() => {
    invoke<PendingPurchase | null>("get_pending_purchase")
      .then(setPending)
      .catch(() => setPending(null));
  }, []);

  const start = useCallback(async (input: PurchaseInput) => {
    setIsStarting(true);
    setError(null);
    try {
      const next = await invoke<PendingPurchase>("create_purchase", { input });
      setPending(next);
      setStatus({ state: "checkout_pending", amount: next.amount, currency: next.currency, activated: false });
      return next;
    } catch (cause) {
      const message = String(cause);
      setError(message);
      throw cause;
    } finally {
      setIsStarting(false);
    }
  }, []);

  const resume = useCallback(async () => {
    setIsOpening(true);
    setError(null);
    try {
      const next = await invoke<PendingPurchase>("resume_purchase_checkout");
      setPending(next);
      return next;
    } catch (cause) {
      setError(String(cause));
      throw cause;
    } finally {
      setIsOpening(false);
    }
  }, []);

  const poll = useCallback(async (quiet = false) => {
    if (!pending?.purchaseId) return null;
    try {
      const next = await invoke<PurchaseStatus>("poll_purchase_status");
      setStatus(next);
      if (!quiet) setError(null);
      if (next.activated) onActivatedRef.current();
      return next;
    } catch (cause) {
      if (!quiet) setError(String(cause));
      return null;
    }
  }, [pending?.purchaseId]);

  useEffect(() => {
    if (!pending?.purchaseId || status?.activated) return;
    void poll(true);
    const timer = window.setInterval(() => void poll(true), 30_000);
    return () => window.clearInterval(timer);
  }, [pending?.purchaseId, poll, status?.activated]);

  const resend = useCallback(async () => {
    setError(null);
    try {
      await invoke("resend_purchase_license");
    } catch (cause) {
      setError(String(cause));
      throw cause;
    }
  }, []);

  const reconcile = useCallback(async () => {
    if (!pending?.purchaseId) return null;
    setError(null);
    try {
      const next = await invoke<PurchaseStatus>("reconcile_purchase_status");
      setStatus(next);
      if (next.activated) onActivatedRef.current();
      return next;
    } catch (cause) {
      setError(String(cause));
      return null;
    }
  }, [pending?.purchaseId]);

  const forget = useCallback(async () => {
    await invoke("forget_pending_purchase");
    setPending(null);
    setStatus(null);
    setError(null);
  }, []);

  const cancelSubscription = useCallback(async () => {
    setIsCancelling(true);
    setError(null);
    try {
      const effectiveAt = await invoke<number | null>("cancel_purchase_subscription");
      setCancellationEffectiveAt(effectiveAt);
      return effectiveAt;
    } catch (cause) {
      setError(String(cause));
      throw cause;
    } finally {
      setIsCancelling(false);
    }
  }, []);

  return { pending, status, isStarting, isOpening, isCancelling, cancellationEffectiveAt, error, start, resume, poll, reconcile, resend, forget, cancelSubscription };
}
