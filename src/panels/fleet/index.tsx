// Fleet Panel — endpoint enrollment plus local Access Control and Vault manifests.
// Access Control owns reusable Windows-user membership. Feature tabs reference
// stable group IDs without duplicating the Windows-user directory.

import { useCallback, useEffect, useRef, useState, type SetStateAction } from "react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import useVaultAccess from "@/hooks/useVaultAccess";
import AccessControlTab from "./AccessControlTab";
import FleetConnectView from "./FleetConnectView";
import VaultAccessTab from "./VaultAccessTab";
import { loadAccessDirectory, saveAccessDirectory } from "./accessControlPolicy";
import type { FleetAccessDirectory } from "./accessControlTypes";
import "./index.css";

export default function FleetPanel() {
  const [capabilityState, setCapabilityState] = useState<"checking" | "admin" | "member" | "unavailable">("checking");
  const [capabilityRefresh, setCapabilityRefresh] = useState(0);
  const [activeTab, setActiveTab] = useState("vault");
  const lastCapability = useRef<boolean | null>(null);
  const { getCapabilities } = useVaultAccess<never, never>();
  const [directory, setDirectory] = useState<FleetAccessDirectory>(loadAccessDirectory);
  const isAdmin = capabilityState === "admin";

  useEffect(() => {
    let active = true;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;

    const probe = async (retriesRemaining: number) => {
      try {
        const capabilities = await getCapabilities();
        if (!active) return;
        const canManage = capabilities.can_manage_policy;
        if (canManage && lastCapability.current !== true) setActiveTab("enrollment");
        if (!canManage) setActiveTab("vault");
        lastCapability.current = canManage;
        setCapabilityState(canManage ? "admin" : "member");
      } catch {
        if (!active) return;
        if (retriesRemaining > 0) {
          retryTimer = setTimeout(() => { void probe(retriesRemaining - 1); }, 750);
        } else {
          setActiveTab("vault");
          setCapabilityState("unavailable");
        }
      }
    };

    const refreshOnFocus = () => { void probe(2); };
    void probe(2);
    window.addEventListener("focus", refreshOnFocus);
    return () => {
      active = false;
      if (retryTimer) clearTimeout(retryTimer);
      window.removeEventListener("focus", refreshOnFocus);
    };
  }, [capabilityRefresh, getCapabilities]);

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
          <h2>{isAdmin ? "Fleet & multi-user security" : capabilityState === "member" ? "My vaults" : "Fleet & Vault access"}</h2>
          <p>{isAdmin ? "Enroll this endpoint, organize Windows users, and assign Vault permissions." : capabilityState === "member" ? "Mount only the Vaults this Windows account is authorized to use." : "Checking this Windows account with the local security service."}</p>
        </div>
      </div>
      {capabilityState === "unavailable" && (
        <div className="fleet-action-row" role="alert">
          <span>The local security service could not confirm this account's Fleet permissions.</span>
          <Button variant="outline" size="sm" onClick={() => {
            setCapabilityState("checking");
            setCapabilityRefresh(current => current + 1);
          }}>Retry permission check</Button>
        </div>
      )}
      <Tabs value={activeTab} onValueChange={setActiveTab} className="fleet-tabs">
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
