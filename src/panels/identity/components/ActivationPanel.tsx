import { useEffect, useState } from "react";
import { Button, Icon } from "@/components/ui/bp";
import { open } from "@tauri-apps/plugin-shell";
import useBackend from "../../../hooks/useBackend";
import { useLicenseQuery } from "../../../hooks/queries/useLicenseQuery";
import './ActivationPanel.css';

// Office Home Premium Retail (O365HomePremRetail x64 en-us). The
// online installer is a small bootstrapper that pulls the latest
// build from Microsoft's CDN; the offline .img is the full payload
// frozen at a release version. Both URLs are official Microsoft
// endpoints (officeapps.live.com / officecdn.microsoft.com).
const OFFICE_ONLINE_URL =
  "https://c2rsetup.officeapps.live.com/c2r/download.aspx?ProductreleaseID=O365HomePremRetail&platform=x64&language=en-us&version=O16GA";
const OFFICE_OFFLINE_URL =
  "https://officecdn.microsoft.com/db/492350f6-3a01-4f97-b9c0-c7c6ddf67d60/media/en-us/O365HomePremRetail.img";

function ActivationPanel() {
  const { getActivationStatus, openActivationSettings, error } = useBackend();
  const { data: license, isLoading: isLicenseLoading } = useLicenseQuery();
  const [officeInstalled, setOfficeInstalled] = useState<boolean>();
  const hasProAccess =
    (license?.licensed === true && license.valid === true) ||
    license?.trial_active === true;

  useEffect(() => {
    // This status read is handled by the Pro sidecar. Do not run it while the
    // licence state is unknown or unavailable: otherwise the initial failed
    // request leaves an entitlement error visible after a trial is activated.
    if (isLicenseLoading || !hasProAccess) {
      setOfficeInstalled(undefined);
      return;
    }

    let active = true;
    void getActivationStatus().then((result) => {
      if (active && result.success && result.data) setOfficeInstalled(result.data.office.installed);
    });
    return () => { active = false; };
  }, [getActivationStatus, hasProAccess, isLicenseLoading]);

  const openWindowsSettings = async () => {
    await openActivationSettings();
  };

  // Tauri plugin-shell's open() routes URLs to the system browser.
  const openExternal = (url: string) => {
    void open(url);
  };

  return (
    <div className="activation-panel">
      {hasProAccess && error && (
        <p className="mb-4 p-2 bg-red-500/10 text-red-400 text-xs rounded border border-red-500/20">
          {error}
        </p>
      )}

      <div className="space-y-4">
        <div className="activation-action-row">
          <div className="action-details">
            <div className="action-title">
              <Icon icon="cog" size={12} className="text-accent" />
              SYSTEM SETTINGS
            </div>
            <div className="action-desc">Open the native Windows Activation settings panel for manual key entry or troubleshooting.</div>
          </div>
          <Button
            text="Open Settings"
            onClick={openWindowsSettings}
          />
        </div>

        {/* Office download row -- both links are official Microsoft
            endpoints for Office Home Premium Retail (O365HomePremRetail
            x64 en-us). Online installer is a small bootstrapper that
            pulls the latest build; offline .img is a full payload
            frozen at a release version. Users supply their own valid
            Microsoft licence and activate via Windows Settings. */}
        {officeInstalled === false && <div className="activation-action-row">
          <div className="action-details">
            <div className="action-title">
              <Icon icon="download" size={12} className="text-accent" />
              MICROSOFT OFFICE
            </div>
            <div className="action-desc">
              Download the official Office 365 Home Premium Retail online or offline installers.
            </div>
          </div>
          <div className="activation-buttons-group">
            <Button
              text="Online (Latest)"
              icon="cloud-download"
              onClick={() => openExternal(OFFICE_ONLINE_URL)}
              title="Small bootstrapper -- downloads + installs latest Office build"
            />
            <Button
              text="Offline (~5 GB)"
              icon="archive"
              onClick={() => openExternal(OFFICE_OFFLINE_URL)}
              title="Full .img payload -- mount and run setup.exe for air-gapped installs"
            />
          </div>
        </div>}
      </div>
    </div>
  );
}

export default ActivationPanel;
