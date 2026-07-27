// src/panels/apps/index.tsx
import { useEffect } from "react";
import AppInstallerPanel from "./components/AppInstallerPanel";
import EnginesSection from "./components/EnginesSection";
import ClassicWindowsApps from "./components/ClassicWindowsApps";
import DebloatPanel from "./DebloatPanel";
import { PackageUpdateTools } from "./PackageUpdateTools";
import PanelHeader from "../../components/shared/PanelHeader";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import { useAppsSessionState } from "./appsSessionState";
import './index.css';

declare global {
  interface Window {
    __pendingAppsPackageUpdates?: boolean;
    __pendingAppInstall?: string[];
  }
}

export default function AppsPanel() {
  const [activeTab, setActiveTab] = useAppsSessionState("apps.active-tab", "install");

  useEffect(() => {
    const scrollToUpdates = () => {
      window.__pendingAppsPackageUpdates = undefined;
      setActiveTab("updates");
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
    <div className="panel-container">
      <div className="flex flex-col gap-6">
        <PanelHeader
          panelId="apps"
          title="Packages & Apps"
          description="Install, update, and remove software in bulk — and strip out the bloatware that ships with Windows."
        />
        <Tabs value={activeTab} onValueChange={setActiveTab}>
          <TabsList className="w-full flex-wrap justify-start">
            <TabsTrigger value="install">Install software</TabsTrigger>
            <TabsTrigger value="updates">Package updates</TabsTrigger>
            <TabsTrigger value="engines">Engines</TabsTrigger>
            <TabsTrigger value="classic">Classic Windows apps</TabsTrigger>
            <TabsTrigger value="debloat">Debloat</TabsTrigger>
          </TabsList>
          <TabsContent value="install"><AppInstallerPanel /></TabsContent>
          <TabsContent value="updates"><PackageUpdateTools /></TabsContent>
          <TabsContent value="engines"><EnginesSection /></TabsContent>
          <TabsContent value="classic"><ClassicWindowsApps /></TabsContent>
          <TabsContent value="debloat"><DebloatPanel /></TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
