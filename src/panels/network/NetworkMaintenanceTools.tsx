// NetworkMaintenanceTools — composition only. Two diagnostics cards, each
// owning its own state hook and presentation:
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

export function NetworkMaintenanceTools() {
  const arp = useArpMaintenance();
  const firewall = useFirewallAudit();

  return (
    <div className="flex flex-col gap-4">
      <ArpCacheCard arp={arp} />
      <FirewallAuditCard firewall={firewall} />
    </div>
  );
}
