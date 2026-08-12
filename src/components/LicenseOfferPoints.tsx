import type { CatalogOffer } from "../hooks/usePurchase";

/**
 * Split the catalogue's policy prose into scannable points.
 *
 * WHY: `detail` and `deviceRule` are the server's authoritative wording, so the
 * client must not paraphrase them (the old client-side OFFER_PROMISES map said
 * the same thing a third time and drifted from the catalogue). Sentence ends and
 * semicolon clauses are the natural point boundaries. The `(?=[A-Z])` guard
 * keeps abbreviations and decimals from splitting mid-sentence.
 */
export function offerPoints(offer: CatalogOffer): string[] {
  return [offer.detail, offer.deviceRule]
    .filter((text): text is string => Boolean(text && text.trim()))
    .flatMap((text) => text.split(/;\s+|(?<=\.)\s+(?=[A-Z])/))
    .map((part) => part.trim().replace(/[.;]+$/, ""))
    .filter(Boolean);
}

/** The selected plan's terms, rendered once, as points rather than a paragraph. */
export default function LicenseOfferPoints({ offer }: { offer: CatalogOffer }) {
  const points = offerPoints(offer);
  if (!points.length) return null;

  return (
    <ul className="license-gate-points" aria-label={`What ${offer.name} includes`}>
      {points.map((point) => (
        <li key={point}>{point}</li>
      ))}
    </ul>
  );
}
