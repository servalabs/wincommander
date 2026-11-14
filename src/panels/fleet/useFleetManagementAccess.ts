import { useEffect, useState } from "react";
import useProcessElevation from "@/hooks/useProcessElevation";
import useVaultAccess from "@/hooks/useVaultAccess";

/** Both the window's elevated token and the service's authorization must agree. */
export function useFleetManagementAccess() {
  const { state: elevation } = useProcessElevation();
  const { getCapabilities } = useVaultAccess<never, never>();
  const [capability, setCapability] = useState<"checking" | "allowed" | "denied" | "unavailable">("checking");
  useEffect(() => {
    let active = true;
    if (elevation !== "administrator") {
      setCapability("checking");
      return () => { active = false; };
    }
    let request = 0;
    const refresh = () => {
      const current = ++request;
      setCapability("checking");
      void getCapabilities().then(result => {
        if (active && current === request) setCapability(result.can_manage_policy === true ? "allowed" : "denied");
      }).catch(() => { if (active && current === request) setCapability("unavailable"); });
    };
    refresh();
    window.addEventListener("focus", refresh);
    return () => { active = false; window.removeEventListener("focus", refresh); };
  }, [elevation, getCapabilities]);
  return { canManage: elevation === "administrator" && capability === "allowed", elevation, capability };
}
