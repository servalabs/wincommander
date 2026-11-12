import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import useVaultAccess from "@/hooks/useVaultAccess";
import AccessControlTab from "./AccessControlTab";
import VaultAccessTab from "./VaultAccessTab";
import { loadAccessDirectory } from "./accessControlPolicy";
import { useFleetAccessDirectory } from "./useFleetAccessDirectory";
import { useFleetManagementAccess } from "./useFleetManagementAccess";
import "./index.css";

type ManagementSection = "access-control" | "vault";

/** Shared service-backed editors, shown only to an elevated authorized process. */
export default function FleetManagementSection({ section }: { section: ManagementSection }) {
  const { canManage, elevation, capability } = useFleetManagementAccess();
  const { getAccessDirectory, saveAccessDirectory } = useVaultAccess<never, never>();
  const groups = useFleetAccessDirectory(canManage, section, getAccessDirectory, saveAccessDirectory);
  const [confirmRefresh, setConfirmRefresh] = useState(false);
  const [hasOpened, setHasOpened] = useState(false);
  useEffect(() => { if (canManage) setHasOpened(true); }, [canManage]);

  if (elevation !== "administrator") return null;
  return <>
    {!canManage && <p role="status">{capability === "checking" ? "Checking management permissions…" : "Management permissions could not be confirmed by the local security service."}</p>}
    {(canManage || hasOpened) && <div className="fleet-panel" hidden={!canManage}>
    {section === "vault" ? <VaultAccessTab isAdmin directory={groups.directory} /> : <>
      <div className="fleet-action-row">
        <span>{groups.dirty ? "Unsaved group changes are kept in this window." : "Saved groups are shared across authorized accounts on this PC."}</span>
        <Button size="sm" variant="outline" disabled={groups.loading || groups.saving} onClick={() => {
          if (groups.dirty) setConfirmRefresh(true);
          else void groups.refresh();
        }}>{groups.loading ? "Refreshing groups…" : "Refresh groups"}</Button>
        {confirmRefresh && <><span>Discard this window's unsaved group changes and reload?</span><Button size="sm" variant="danger" onClick={() => { setConfirmRefresh(false); void groups.refresh(true); }}>Discard draft and refresh</Button><Button size="sm" variant="outline" onClick={() => setConfirmRefresh(false)}>Keep draft</Button></>}
        {groups.loaded && groups.directory.groups.length === 0 && loadAccessDirectory().groups.length > 0 && <Button size="sm" variant="outline" onClick={() => groups.update(loadAccessDirectory())}>Import old local draft</Button>}
      </div>
      {groups.error && <p className="fleet-inline-error" role="alert">{groups.error}</p>}
      {groups.loaded ? <AccessControlTab directory={groups.directory} onChange={groups.update} onSave={groups.save} /> : <p role="status">{groups.loading ? "Loading saved groups from this PC…" : "Refresh groups to load the machine's saved directory before editing."}</p>}
    </>}
    </div>}
  </>;
}
