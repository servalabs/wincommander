// ══════════════════════════════════════════════════════════════════════════
// Fleet Panel — device enrollment plus a local Vault deployment manifest.
//
// The fleet ADMIN console (Devices / Control / Policy / Commands / Duress /
// Groups / Admins / Audit / …) now lives in ONE web app served by the
// fleet-server itself (`wincommander-pro/fleet-server/console`, reachable at
// the server's own origin, e.g. http://fleet.corp.ts.net:8787). WinCommander's
// only remote-facing action here is enrolling THIS device (see FleetConnectView
// → `fleet_connect`). The Vault tab is deliberately local: it creates a
// non-secret deployment manifest and does not log in to, or manage, the server.
// ══════════════════════════════════════════════════════════════════════════

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import FleetConnectView from "./FleetConnectView";
import VaultAccessTab from "./VaultAccessTab";
import "./index.css";

export default function FleetPanel() {
  return (
    <div className="panel-container fleet-panel">
      <div className="fleet-panel-heading">
        <div>
          <h2>Fleet & multi-user security</h2>
          <p>Enroll this endpoint or prepare a local, non-secret Vault deployment manifest.</p>
        </div>
      </div>
      <Tabs defaultValue="enrollment" className="fleet-tabs">
        <TabsList className="fleet-tabs-list" aria-label="Fleet configuration sections">
          <TabsTrigger value="enrollment">Enrollment</TabsTrigger>
          <TabsTrigger value="vault">Vault access</TabsTrigger>
        </TabsList>
        <TabsContent value="enrollment" className="fleet-tab-content">
          <FleetConnectView />
          <p className="fleet-section-sub" style={{ marginTop: 20 }}>
            Manage remote fleet controls from the web console served by your fleet-server.
          </p>
        </TabsContent>
        <TabsContent value="vault" className="fleet-tab-content">
          <VaultAccessTab />
        </TabsContent>
      </Tabs>
    </div>
  );
}
