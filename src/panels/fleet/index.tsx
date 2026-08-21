// Fleet Panel — endpoint enrollment plus local Access Control and Vault manifests.
// Access Control owns reusable Windows-user membership. Feature tabs reference
// stable group IDs without duplicating the Windows-user directory.

import { useCallback, useState, type SetStateAction } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import AccessControlTab from "./AccessControlTab";
import FleetConnectView from "./FleetConnectView";
import VaultAccessTab from "./VaultAccessTab";
import { loadAccessDirectory, saveAccessDirectory } from "./accessControlPolicy";
import type { FleetAccessDirectory } from "./accessControlTypes";
import "./index.css";

export default function FleetPanel() {
  const [directory, setDirectory] = useState<FleetAccessDirectory>(loadAccessDirectory);

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
          <h2>Fleet & multi-user security</h2>
          <p>Enroll this endpoint, organize Windows users, and assign Vault permissions.</p>
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
        </TabsContent>
        <TabsContent value="access-control" className="fleet-tab-content">
          <AccessControlTab directory={directory} onChange={updateDirectory} onSave={saveDirectory} />
        </TabsContent>
        <TabsContent value="vault" className="fleet-tab-content">
          <VaultAccessTab />
        </TabsContent>
      </Tabs>
    </div>
  );
}
