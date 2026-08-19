// Fleet Panel — endpoint enrollment plus local Access Control and Vault manifests.
// Access Control owns reusable Windows-user membership. Feature tabs reference
// stable group IDs without duplicating the Windows-user directory.

import { useCallback, useState, type SetStateAction } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import AccessControlTab from "./AccessControlTab";
import FleetConsoleLink from "./FleetConsoleLink";
import FleetConnectView from "./FleetConnectView";
import VaultAccessTab from "./VaultAccessTab";
import { loadAccessDirectory, saveAccessDirectory } from "./accessControlPolicy";
import type { FleetAccessDirectory } from "./accessControlTypes";
import type { VaultFleetPolicy } from "./vaultFleetTypes";
import { loadVaultPolicy, saveVaultPolicy } from "./vaultFleetPolicy";
import "./index.css";

export default function FleetPanel() {
  const [directory, setDirectory] = useState<FleetAccessDirectory>(loadAccessDirectory);
  const [vaultPolicy, setVaultPolicy] = useState<VaultFleetPolicy>(loadVaultPolicy);

  const updateDirectory = useCallback((action: SetStateAction<FleetAccessDirectory>) => {
    setDirectory(current => {
      const next = typeof action === "function" ? action(current) : action;
      const nextIds = new Set(next.groups.map(group => group.id));
      const removedIds = new Set(current.groups.filter(group => !nextIds.has(group.id)).map(group => group.id));
      if (removedIds.size > 0) {
        setVaultPolicy(policy => ({
          ...policy,
          volumes: policy.volumes.map(volume => ({
            ...volume,
            groupPermissions: Object.fromEntries(
              Object.entries(volume.groupPermissions).filter(([groupId]) => !removedIds.has(groupId)),
            ),
          })),
        }));
      }
      return next;
    });
  }, []);

  const saveDirectory = () => {
    saveAccessDirectory(directory);
    saveVaultPolicy(vaultPolicy);
  };

  return (
    <div className="panel-container fleet-panel">
      <div className="fleet-panel-heading">
        <div>
          <h2>Fleet & multi-user security</h2>
          <p>Enroll this endpoint, organize Windows users, assign Vault permissions, or open the organization console.</p>
        </div>
      </div>
      <Tabs defaultValue="enrollment" className="fleet-tabs">
        <TabsList className="fleet-tabs-list" aria-label="Fleet configuration sections">
          <TabsTrigger value="enrollment">Enrollment</TabsTrigger>
          <TabsTrigger value="access-control">Access control</TabsTrigger>
          <TabsTrigger value="vault">Vault permissions</TabsTrigger>
        </TabsList>
        <TabsContent value="enrollment" className="fleet-tab-content">
          <FleetConnectView />
          <FleetConsoleLink />
        </TabsContent>
        <TabsContent value="access-control" className="fleet-tab-content">
          <AccessControlTab directory={directory} onChange={updateDirectory} onSave={saveDirectory} />
        </TabsContent>
        <TabsContent value="vault" className="fleet-tab-content">
          <VaultAccessTab
            directory={directory}
            policy={vaultPolicy}
            onChange={setVaultPolicy}
            onSave={() => saveVaultPolicy(vaultPolicy)}
          />
        </TabsContent>
      </Tabs>
    </div>
  );
}
