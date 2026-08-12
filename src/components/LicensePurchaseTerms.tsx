import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";

/**
 * Trust and compliance copy that used to occupy three full-width blocks in the
 * purchase surface. The wording is unchanged — only its placement moved, so the
 * path to payment stays scroll-free while the statements remain one click away.
 */
export default function LicensePurchaseTerms({
  hasInvestigatorEntitlement,
}: {
  hasInvestigatorEntitlement: boolean;
}) {
  return (
    <Popover>
      <PopoverTrigger className="license-gate-terms-trigger" aria-label="Pricing and payment details">
        <span className="license-gate-terms-glyph" aria-hidden>i</span>
        Details
      </PopoverTrigger>
      {/* The gate overlay sits at z-index 10500 and the popover portals to
          <body>, so it must clear that or it renders behind the modal. */}
      <PopoverContent align="end" className="license-gate-terms z-[10600] w-[330px]">
        <p>
          <strong>Free stays free</strong> The core WinCommander app works without a licence or
          payment. Upgrade only for Pro capabilities, services, or managed Fleet endpoints.
        </p>
        <p>The secure checkout service confirms the current price before payment.</p>
        <p>
          Checkout opens in your normal browser. WinCommander never handles payment credentials.
          Pro Lifetime keeps normal Pro usable permanently; memberships retain the last eligible
          normal Pro build if they end. Netwall is a hosted, term-based service.
          {hasInvestigatorEntitlement
            ? " Investigator is available only while its explicit entitlement is active."
            : ""}
        </p>
      </PopoverContent>
    </Popover>
  );
}
