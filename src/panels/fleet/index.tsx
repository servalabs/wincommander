// Personal Vault access remains available from the existing Fleet navigation.
// Administrative setup lives in Windows Settings and Secure Storage.
import VaultAccessTab from "./VaultAccessTab";
import { DEFAULT_ACCESS_DIRECTORY } from "./accessControlPolicy";
import "./index.css";

export default function FleetPanel() {
  return <div className="panel-container fleet-panel">
    <div className="fleet-panel-heading">
      <div><h2>My vaults</h2><p>Mount the Vaults this Windows account is authorized to use.</p></div>
    </div>
    <VaultAccessTab isAdmin={false} directory={DEFAULT_ACCESS_DIRECTORY} />
  </div>;
}
