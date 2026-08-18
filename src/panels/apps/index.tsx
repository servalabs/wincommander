// src/panels/apps/index.tsx
import { useEffect, useState } from "react";
import AppInstallerPanel, { type AppInstallerStatus } from "./components/AppInstallerPanel";
import DebloatPanel from "./DebloatPanel";
import { PackageUpdateTools } from "./PackageUpdateTools";
import PanelHeader from "../../components/shared/PanelHeader";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import { Icon, Spinner } from "../../components/ui/bp";
import { useAppsSessionState } from "./appsSessionState";
import './index.css';

declare global {
  interface Window {
    __pendingAppsPackageUpdates?: boolean;
    __pendingAppInstall?: string[];
    __pendingAppsInstallView?: "updates";
  }
}

export default function AppsPanel() {
  const [activeTab, setActiveTab] = useAppsSessionState("apps.active-tab", "install");
  const [installerStatus, setInstallerStatus] = useState<AppInstallerStatus>({
    wingetStatus: "checking",
    vulnerabilityTone: "is-neutral",
    vulnerabilityText: "SCANNING APP INVENTORY",
  });

  useEffect(() => {
    const scrollToUpdates = () => {
      window.__pendingAppsPackageUpdates = undefined;
      window.__pendingAppsInstallView = "updates";
      setActiveTab("install");
      window.dispatchEvent(new Event("apps-open-updates-tab"));
      window.setTimeout(() => document.getElementById("package-updates")?.scrollIntoView({ behavior: "smooth", block: "start" }), 80);
    };
    // Deep-link: an install request fired while the user is elsewhere must not
    // be dropped just because AppInstallerPanel's tab isn't mounted — stash it
    // on window and switch tabs; AppInstallerPanel's own mount-effect already
    // reads window.__pendingAppInstall once its tab (re)mounts.
    const handleAppsInstallMissing = (event: Event) => {
      const detail = (event as CustomEvent<{ appIds?: string[] }>).detail;
      if (detail?.appIds?.length) window.__pendingAppInstall = detail.appIds;
      setActiveTab("install");
    };
    const openInstallTab = () => setActiveTab("install");

    window.addEventListener("apps-package-updates-ready", scrollToUpdates);
    window.addEventListener("apps-install-missing", handleAppsInstallMissing as EventListener);
    window.addEventListener("apps-open-install-tab", openInstallTab);
    // The source panel is lazy-loaded.  Retain the request on window so the
    // first navigation cannot lose its one-shot event before this effect runs.
    if (window.__pendingAppsPackageUpdates) requestAnimationFrame(scrollToUpdates);
    return () => {
      window.removeEventListener("apps-package-updates-ready", scrollToUpdates);
      window.removeEventListener("apps-install-missing", handleAppsInstallMissing as EventListener);
      window.removeEventListener("apps-open-install-tab", openInstallTab);
    };
  }, [setActiveTab]);

  return (
    <div className="panel-container apps-panel">
      <div className="flex flex-col gap-0">
        <PanelHeader
          panelId="apps"
          title="Packages & Apps"
          description="Install, update, and remove software in bulk — and strip out the bloatware that ships with Windows."
        />
        <Tabs value={activeTab} onValueChange={setActiveTab}>
          <TabsList className="apps-panel-tabs w-full flex-wrap justify-start">
            <TabsTrigger value="install">Install software</TabsTrigger>
            <TabsTrigger value="debloat">Debloat</TabsTrigger>
            <div className="apps-panel-tabs__status">
              <span className={`winget-status ${installerStatus.wingetStatus === "installed" ? "installed" : installerStatus.wingetStatus === "not-installed" ? "not-installed" : ""}`}>
                {installerStatus.wingetStatus === "checking" && <Spinner size={14} />}
                {installerStatus.wingetStatus === "installed" && <><Icon icon="tick-circle" size={14} /> PACKAGE MANAGER INSTALLED</>}
                {installerStatus.wingetStatus === "not-installed" && <><Icon icon="cross-circle" size={14} /> PACKAGE MANAGER MISSING</>}
                {installerStatus.wingetStatus === "installing" && <><Spinner size={14} /> INSTALLING...</>}
                {installerStatus.wingetStatus === "failed" && <><Icon icon="error" size={14} /> PACKAGE MANAGER FAILED</>}
              </span>
              <span className={`vulnerability-badge ${installerStatus.vulnerabilityTone}`}>
                <Icon icon={installerStatus.vulnerabilityTone === "is-safe" ? "shield" : installerStatus.vulnerabilityTone === "is-risk" ? "warning-sign" : installerStatus.vulnerabilityTone === "is-warning" ? "issue" : "info-sign"} size={12} />
                <span>{installerStatus.vulnerabilityText}</span>
              </span>
            </div>
          </TabsList>
          <TabsContent value="install"><AppInstallerPanel onStatusChange={setInstallerStatus} updatesTools={<PackageUpdateTools />} /></TabsContent>
          <TabsContent value="debloat"><DebloatPanel /></TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
