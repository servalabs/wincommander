// NetworkMaintenanceTools — composition only. The wider neighbour table and
// the compact adapters card share the first row; Firewall Audit gets the full
// row underneath so neither table is squeezed into a narrow side column.
// Each diagnostics card owns its own state hook and presentation:
//   · ArpCacheCard      — arp_cache_scan / arp_cache_clear
//   · FirewallAuditCard — firewall_audit_preview / _remediate / _cancel
//
// The two former "Adapter diagnostics" + "Neighbor mappings" cards and the
// floating clear button were one feature split across three surfaces; they are
// now a single ARP card. Everything here uses the shadcn/v2 kit — no bp imports
// in this subtree.
import { ArpCacheCard } from "./ArpCacheCard";
import { FirewallAuditCard } from "./FirewallAuditCard";
import { useArpMaintenance } from "./useArpMaintenance";
import { useFirewallAudit } from "./useFirewallAudit";
import type { ReactNode } from "react";

export function NetworkMaintenanceTools({ adapterControls }: { adapterControls: ReactNode }) {
  const arp = useArpMaintenance();
  const firewall = useFirewallAudit();

  return (
    <div className="network-maintenance-layout">
      <div className="network-maintenance-layout__top">
        <ArpCacheCard arp={arp} />
        <div className="network-maintenance-layout__adapters">{adapterControls}</div>
      </div>
      <FirewallAuditCard firewall={firewall} />
    </div>
  );
}
