// Fleet Panel — endpoint enrollment plus local Access Control and Vault manifests.
// Access Control owns reusable Windows-user membership. Feature tabs reference
// stable group IDs without duplicating the Windows-user directory.

import { useCallback, useEffect, useState, type SetStateAction } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import useVaultAccess from "@/hooks/useVaultAccess";
import AccessControlTab from "./AccessControlTab";
import FleetConnectView from "./FleetConnectView";
import VaultAccessTab from "./VaultAccessTab";
import { loadAccessDirectory, saveAccessDirectory } from "./accessControlPolicy";
import type { FleetAccessDirectory } from "./accessControlTypes";
import "./index.css";

export default function FleetPanel() {
  const [isAdmin, setIsAdmin] = useState(false);
  const { getCapabilities } = useVaultAccess<never, never>();
  const [directory, setDirectory] = useState<FleetAccessDirectory>(loadAccessDirectory);

  useEffect(() => {
    let active = true;
    void getCapabilities()
      .then(capabilities => { if (active) setIsAdmin(capabilities.can_manage_policy); })
      .catch(() => { if (active) setIsAdmin(false); });
    return () => { active = false; };
  }, [getCapabilities]);

  const updateDirectory = useCallback((action: SetStateAction<FleetAccessDirectory>) => {
    setDirectory(current => {
      const next = typeof action === "function" ? action(current) : action;
      return next;
    });
  }, []);

  const saveDirectory = () => {
    saveAccessDirectory(directory);
  };

  return (
    <div className="panel-container fleet-panel">
      <div className="fleet-panel-heading">
        <div>
          <h2>{isAdmin ? "Fleet & multi-user security" : "My vaults"}</h2>
          <p>{isAdmin ? "Enroll this endpoint, organize Windows users, and assign Vault permissions." : "Mount only the Vaults this Windows account is authorized to use."}</p>
        </div>
      </div>
      <Tabs defaultValue={isAdmin ? "enrollment" : "vault"} className="fleet-tabs">
        <TabsList className="fleet-tabs-list" aria-label="Fleet configuration sections">
          {isAdmin && <TabsTrigger value="enrollment">Enrollment</TabsTrigger>}
          {isAdmin && <TabsTrigger value="access-control">Access control</TabsTrigger>}
          <TabsTrigger value="vault">{isAdmin ? "Vault permissions" : "My vaults"}</TabsTrigger>
        </TabsList>
        {isAdmin && <TabsContent value="enrollment" className="fleet-tab-content">
          <FleetConnectView />
        </TabsContent>}
        {isAdmin && <TabsContent value="access-control" className="fleet-tab-content">
          <AccessControlTab directory={directory} onChange={updateDirectory} onSave={saveDirectory} />
        </TabsContent>}
        <TabsContent value="vault" className="fleet-tab-content">
          <VaultAccessTab isAdmin={isAdmin} />
        </TabsContent>
      </Tabs>
    </div>
  );
}
