import { motion } from "framer-motion";
import { Badge } from "@/components/ui/badge";
import { SPRING } from "./shared/motion";
import type { CatalogOffer, PurchaseSku } from "../hooks/usePurchase";

/** Two-word scan cues. Substantive terms live on the selected plan's point list,
 *  never repeated per card. */
const OFFER_BADGES: Record<PurchaseSku, string[]> = {
  pro_lifetime: ["Pay once", "Lifetime updates"],
  pro_membership: ["Subscription", "Netwall included"],
  investigator: ["Subscription", "Netwall included"],
  fleet: ["Subscription", "Netwall included"],
};

interface Props {
  offer: CatalogOffer;
  isSelected: boolean;
  /** Scopes the shared-element ring to one panel instance; framer-motion
   *  layoutIds are document-global, so two mounted gates would fling one ring
   *  between them. */
  ringId: string;
  onSelect: () => void;
}

export default function LicenseOfferCard({ offer, isSelected, ringId, onSelect }: Props) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={isSelected}
      className={`license-gate-offer${isSelected ? " active" : ""}`}
      onClick={onSelect}
    >
      {/* The ring mounts only on the selected card; when it unmounts here and
          mounts on another card, framer animates the shared geometry. Reduced
          motion collapses it to instant via <MotionConfig> in App.tsx. */}
      {isSelected && (
        <motion.span
          layoutId={`license-offer-ring-${ringId}`}
          className="license-gate-offer-ring"
          transition={SPRING.snappy}
          aria-hidden
        />
      )}
      <span className="license-gate-offer-heading">
        <strong>{offer.name}</strong>
        <span>{offer.priceLabel}</span>
      </span>
      <span className="license-gate-offer-badges">
        {OFFER_BADGES[offer.sku].map((badge) => (
          <Badge key={badge} tone={isSelected ? "accent" : "neutral"}>
            {badge}
          </Badge>
        ))}
      </span>
    </button>
  );
}
