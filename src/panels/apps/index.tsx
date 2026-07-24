// src/panels/apps/index.tsx
import { useEffect } from "react";
import AppInstallerPanel from "./components/AppInstallerPanel";
import SectionCard from "../../components/shared/SectionCard";
import DebloatPanel from "./DebloatPanel";
import { PackageUpdateTools } from "../maintenance/PackageUpdateTools";
import './index.css';

declare global {
  interface Window {
    __pendingAppsPackageUpdates?: boolean;
  }
}

export default function AppsPanel() {
  useEffect(() => {
    const scrollToUpdates = () => {
      window.__pendingAppsPackageUpdates = undefined;
      document.getElementById("package-updates")?.scrollIntoView({ behavior: "smooth", block: "start" });
    };
    window.addEventListener("apps-package-updates-ready", scrollToUpdates);
    // The source panel is lazy-loaded.  Retain the request on window so the
    // first navigation cannot lose its one-shot event before this effect runs.
    if (window.__pendingAppsPackageUpdates) requestAnimationFrame(scrollToUpdates);
    return () => {
      window.removeEventListener("apps-package-updates-ready", scrollToUpdates);
    };
  }, []);

  return (
    <div className="panel-container">
      <div className="flex flex-col gap-6">
        <AppInstallerPanel />
        <PackageUpdateTools />
        <SectionCard title="Debloat">
          <DebloatPanel />
        </SectionCard>
      </div>
    </div>
  );
}
