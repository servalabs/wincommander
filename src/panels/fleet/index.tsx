// ══════════════════════════════════════════════════════════════════════════
// Fleet Panel — ENROLL-ONLY.
//
// The fleet ADMIN console (Devices / Control / Policy / Commands / Duress /
// Groups / Admins / Audit / …) now lives in ONE web app served by the
// fleet-server itself (`wincommander-pro/fleet-server/console`, reachable at
// the server's own origin, e.g. http://fleet.corp.ts.net:8787). WinCommander's
// only fleet surface is enrolling THIS device — a Tauri-native action (see
// FleetConnectView → `fleet_connect`). No HTTP admin client ships in the app
// anymore, so the panel no longer repoints at / logs into a remote server.
// ══════════════════════════════════════════════════════════════════════════

import FleetConnectView from "./FleetConnectView";
import "./index.css";

export default function FleetPanel() {
  return (
    <div className="panel-container fleet-panel">
      <div className="fleet-body">
        <div className="fleet-view">
          <FleetConnectView />
          <p className="fleet-section-sub" style={{ marginTop: 20 }}>
            Manage your fleet — devices, policy, commands, and duress controls —
            from the web console served by your fleet-server (e.g.{" "}
            <span className="mono">http://fleet.corp.ts.net:8787</span>). This
            app only enrolls this device.
          </p>
        </div>
      </div>
    </div>
  );
}
