import { Button } from "@/components/ui/bp";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import SectionCard from "../../components/shared/SectionCard";
import UniversalToggle from "../../components/shared/UniversalToggle";
import useBackend, { type InstalledBrowser } from "../../hooks/useBackend";
import { useAppState } from "../../context/AppContext";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import { Switch } from "../../components/ui/switch";
import { showError, showWarning } from "../../utils/toast";
import ManageExtensionsPanel from "./ManageExtensionsPanel";
import { browserExtensionSettingKey } from "../../registry/browserExtensions";
import { browserLogos } from "../../assets";
import { resolveBrowserIconUrl } from "./browserIcons";

interface BrowserHardeningSectionProps {
  isAdvanced: boolean;
  searchQuery: string;
}

interface BrowserRestoreData {
  profileCleanupErrors?: string[];
}

export default function BrowserHardeningSection({ isAdvanced, searchQuery }: BrowserHardeningSectionProps) {
  const {
    getInstalledBrowsers,
    hardenBrowserByName,
    restoreBrowserByName,
  } = useBackend();
  const { appSettings, patchAppSettings } = useAppState();

  const [detectedBrowsers, setDetectedBrowsers] = useState<InstalledBrowser[] | null>(null);
  const [browserStatus, setBrowserStatus] = useState<Record<string, boolean>>({});
  const [browsersLoading, setBrowsersLoading] = useState(false);
  const [browserDetectionSlow, setBrowserDetectionSlow] = useState(false);
  const [browserDetectError, setBrowserDetectError] = useState<string | null>(null);
  const [localLoadingMap, setLocalLoadingMap] = useState<Record<string, boolean>>({});
  const [allBrowsersLoading, setAllBrowsersLoading] = useState(false);
  const [selectedBrowserName, setSelectedBrowserName] = useState<string | null>(null);
  const desiredStatusRef = useRef<Record<string, boolean>>({});
  const extensionToggles = appSettings?.ideal?.privacy?.browserExtensions;

  // Re-apply only the browser whose preferences changed; applying every
  // hardened browser was the source of cross-browser extension changes.
  const handleExtensionToggle = useCallback(async (browser: InstalledBrowser, slug: string, enabled: boolean) => {
    const settingKey = browserExtensionSettingKey(browser.Name, slug);
    const key = `ext_${settingKey}`;
    setLocalLoadingMap((prev) => ({ ...prev, [key]: true }));
    try {
      await patchAppSettings({ ideal: { privacy: { browserExtensions: { [settingKey]: enabled } } } });
      if (browserStatus[browser.Name]) {
        const result = await hardenBrowserByName(browser.Name);
        requireBackendSuccess(result, `Could not re-apply extensions to ${browser.Name}.`);
      }
    } catch (error) {
      showError(error instanceof Error ? error.message : String(error));
    } finally {
      setLocalLoadingMap((prev) => ({ ...prev, [key]: false }));
    }
  }, [browserStatus, patchAppSettings, hardenBrowserByName]);

  const requireBackendSuccess = (result: { success: boolean; error?: string }, fallback: string) => {
    if (!result.success) throw new Error(result.error || fallback);
  };

  const warnProfileCleanupErrors = (browserName: string, data?: BrowserRestoreData | null) => {
    const errors = Array.isArray(data?.profileCleanupErrors) ? data.profileCleanupErrors : [];
    if (errors.length > 0) {
      void showWarning(`${browserName} policy was removed, but some extension files are locked. Close the browser and toggle off again to remove the remaining profile files.`);
    }
  };

  const loadBrowsers = useCallback(async (options?: { showLoading?: boolean }) => {
    const showLoading = options?.showLoading ?? true;
    if (showLoading) {
      setBrowsersLoading(true);
      setBrowserDetectionSlow(false);
      setBrowserDetectError(null);
    }
    const slowTimer = showLoading ? setTimeout(() => setBrowserDetectionSlow(true), 3500) : undefined;
    try {
      const result = await getInstalledBrowsers();
      requireBackendSuccess(result, "Browser detection failed.");
      const list = result.data?.browsers ?? [];
      setDetectedBrowsers(list);
      const nextStatus: Record<string, boolean> = {};
      for (const browser of list) {
        nextStatus[browser.Name] = Object.prototype.hasOwnProperty.call(desiredStatusRef.current, browser.Name)
          ? desiredStatusRef.current[browser.Name]
          : Boolean(browser.Hardened);
      }
      setBrowserStatus(nextStatus);
    } catch (error) {
      if (showLoading) {
        setBrowserDetectError(error instanceof Error ? error.message : "Browser detection failed.");
        setDetectedBrowsers([]);
      }
    } finally {
      if (slowTimer) clearTimeout(slowTimer);
      if (showLoading) {
        setBrowserDetectionSlow(false);
        setBrowsersLoading(false);
      }
    }
  }, [getInstalledBrowsers]);

  const handleBrowserToggle = useCallback(async (browser: InstalledBrowser, checked: boolean) => {
    const key = `browser_${browser.Name}`;
    setLocalLoadingMap((prev) => ({ ...prev, [key]: true }));
    try {
      const result = await (checked ? hardenBrowserByName(browser.Name) : restoreBrowserByName(browser.Name));
      requireBackendSuccess(result, checked ? `Could not harden ${browser.Name}.` : `Could not restore ${browser.Name}.`);
      if (!checked) warnProfileCleanupErrors(browser.Name, result.data as BrowserRestoreData | undefined);
      desiredStatusRef.current = { ...desiredStatusRef.current, [browser.Name]: checked };
      setBrowserStatus((prev) => ({ ...prev, [browser.Name]: checked }));
    } catch (error) {
      showError(error instanceof Error ? error.message : String(error));
    } finally {
      await loadBrowsers({ showLoading: false });
      setLocalLoadingMap((prev) => ({ ...prev, [key]: false }));
    }
  }, [hardenBrowserByName, restoreBrowserByName, loadBrowsers]);

  const handleAllBrowsersToggle = useCallback(async (checked: boolean) => {
    const browsers = detectedBrowsers ?? [];
    if (browsers.length === 0) return;

    setAllBrowsersLoading(true);
    const nextStatus: Record<string, boolean> = {};
    const failures: string[] = [];

    try {
      // Apply in order so each browser's policy/profile update has completed
      // before the next one starts. This avoids competing extension writes.
      for (const browser of browsers) {
        if (Boolean(browserStatus[browser.Name]) === checked) continue;

        try {
          const result = await (checked ? hardenBrowserByName(browser.Name) : restoreBrowserByName(browser.Name));
          requireBackendSuccess(result, checked ? `Could not harden ${browser.Name}.` : `Could not restore ${browser.Name}.`);
          if (!checked) warnProfileCleanupErrors(browser.Name, result.data as BrowserRestoreData | undefined);
          nextStatus[browser.Name] = checked;
        } catch (error) {
          failures.push(error instanceof Error ? error.message : String(error));
        }
      }

      if (Object.keys(nextStatus).length > 0) {
        desiredStatusRef.current = { ...desiredStatusRef.current, ...nextStatus };
        setBrowserStatus((previous) => ({ ...previous, ...nextStatus }));
      }
      if (failures.length > 0) showError(failures.join(" "));
    } finally {
      await loadBrowsers({ showLoading: false });
      setAllBrowsersLoading(false);
    }
  }, [browserStatus, detectedBrowsers, hardenBrowserByName, loadBrowsers, restoreBrowserByName]);

  useEffect(() => {
    void loadBrowsers({ showLoading: true });
  }, [loadBrowsers]);

  useEffect(() => {
    const browsers = detectedBrowsers ?? [];
    setSelectedBrowserName((current) => (
      current && browsers.some((browser) => browser.Name === current)
        ? current
        : (browsers[0]?.Name ?? null)
    ));
  }, [detectedBrowsers]);

  const selectedBrowser = useMemo(
    () => (detectedBrowsers ?? []).find((browser) => browser.Name === selectedBrowserName) ?? null,
    [detectedBrowsers, selectedBrowserName],
  );

  const allBrowsersHardened = (detectedBrowsers?.length ?? 0) > 0 && (detectedBrowsers ?? []).every(
    (browser) => Boolean(browserStatus[browser.Name]),
  );

  const matchesSearch = useMemo(() => {
    const q = searchQuery.toLowerCase().trim();
    if (!q) return true;

    const sectionTerms = [
      "browser",
      "browsers",
      "browser hardening",
      "secure browsers",
      "extensions",
      "telemetry",
      "firefox",
      "chrome",
      "edge",
      "brave",
    ];

    if (sectionTerms.some((term) => term.includes(q))) return true;

    return (detectedBrowsers ?? []).some((browser) => {
      const label = isAdvanced ? `harden ${browser.Name}` : `secure ${browser.Name}`;
      const description = browser.Engine === "Gecko"
        ? "Telemetry off, tracking blocked, extensions"
        : "Telemetry off, sync/ads disabled, extensions";
      return label.toLowerCase().includes(q) || description.toLowerCase().includes(q) || browser.Name.toLowerCase().includes(q);
    });
  }, [detectedBrowsers, isAdvanced, searchQuery]);

  if (!matchesSearch) return null;

  return (
    <SectionCard
      title={isAdvanced ? "Browser Hardening" : "Secure Browsers"}
      className="w-full"
      armed={allBrowsersHardened}
      headerRight={
        <label className="flex items-center gap-2 whitespace-nowrap text-xs font-semibold text-[var(--color-text-muted)]">
          <span>{isAdvanced ? "Harden all" : "Secure all"}</span>
          <Switch
            checked={allBrowsersHardened}
            disabled={browsersLoading || allBrowsersLoading || !detectedBrowsers?.length}
            onCheckedChange={(checked) => void handleAllBrowsersToggle(checked)}
            aria-label={allBrowsersHardened ? "Restore all browsers" : "Harden all browsers"}
          />
        </label>
      }
    >
      <div data-tour="privacy-browser-hardening" className="flex flex-col gap-4">
        {browsersLoading && (
          <div className="text-sm opacity-50">
            {browserDetectionSlow ? "Still checking installed browsers and hardening status..." : "Detecting installed browsers..."}
          </div>
        )}
        {!browsersLoading && browserDetectError && (
          <div role="alert" className="flex items-center justify-between gap-3 rounded border border-[var(--color-warning)]/40 bg-[var(--color-warning)]/10 px-3 py-2">
            <span className="text-xs text-[var(--color-warning)]">{browserDetectError}</span>
            <Button small minimal icon="refresh" onClick={() => void loadBrowsers({ showLoading: true })}>Retry</Button>
          </div>
        )}
        {!browsersLoading && !browserDetectError && detectedBrowsers?.length === 0 && (
          <div className="text-sm opacity-50">No supported browsers detected.</div>
        )}
        {!browsersLoading && !browserDetectError && selectedBrowser && (
          <Tabs value={selectedBrowser.Name} onValueChange={setSelectedBrowserName}>
            <TabsList className="w-full flex-wrap justify-start">
              {detectedBrowsers?.map((browser) => (
                <TabsTrigger key={browser.Name} value={browser.Name} className="gap-2">
                  {/* Tailwind's preflight sets img to display:block, which
                      breaks it onto its own anonymous block box when it's a
                      bare child alongside inline text — stacking the logo
                      above the name instead of beside it. Wrapping both in
                      one inline-flex row keeps them horizontal. */}
                  <span className="inline-flex items-center gap-1.5">
                    {resolveBrowserIconUrl(browser.Name, browserLogos) && (
                      <img
                        src={resolveBrowserIconUrl(browser.Name, browserLogos)}
                        alt=""
                        width={18}
                        height={18}
                        className="shrink-0 object-contain"
                      />
                    )}
                    <span>{browser.Name}</span>
                  </span>
                </TabsTrigger>
              ))}
            </TabsList>
            <TabsContent value={selectedBrowser.Name} className="mt-1 flex flex-col gap-4">
              <UniversalToggle
                label={isAdvanced ? `Harden ${selectedBrowser.Name}` : `Secure ${selectedBrowser.Name}`}
                description={selectedBrowser.Engine === "Gecko" ? "Telemetry off, tracking blocked, extensions" : "Telemetry off, sync/ads disabled, extensions"}
                checked={Boolean(browserStatus[selectedBrowser.Name])}
                iconImage={resolveBrowserIconUrl(selectedBrowser.Name, browserLogos)}
                onChange={(checked) => handleBrowserToggle(selectedBrowser, checked)}
                loading={allBrowsersLoading || localLoadingMap[`browser_${selectedBrowser.Name}`]}
                disabled={allBrowsersLoading || localLoadingMap[`browser_${selectedBrowser.Name}`]}
                size="compact"
              />
              <ManageExtensionsPanel
                browserName={selectedBrowser.Name}
                browserEnabled={Boolean(browserStatus[selectedBrowser.Name])}
                extensionToggles={extensionToggles}
                localLoadingMap={localLoadingMap}
                onToggle={(slug, checked) => handleExtensionToggle(selectedBrowser, slug, checked)}
              />
            </TabsContent>
          </Tabs>
        )}
      </div>
    </SectionCard>
  );
}
