// Per-extension toggle grid, rendered inline inside BrowserHardeningSection's
// SectionCard, always visible by default (2026-07-20: product feedback —
// "as this should be shown in the card only by default non additional modal
// pop is required" — the earlier click-to-reveal "Manage Extensions…"
// button and Back-button header are gone; there's no view left to navigate
// back from). Grid rendering is unchanged from the prior click-to-reveal
// version; the toggle logic (re-applying hardening to already-hardened
// browsers immediately) stays owned by the parent and is only invoked via
// the onToggle callback.
import UniversalToggle from "../../components/shared/UniversalToggle";
import {
  BROWSER_EXTENSION_TOGGLES,
  browserExtensionSettingKey,
  isBrowserExtensionEnabled,
} from "../../registry/browserExtensions";

interface ManageExtensionsPanelProps {
  browserName: string;
  browserEnabled: boolean;
  extensionToggles: Record<string, boolean> | undefined;
  localLoadingMap: Record<string, boolean>;
  onToggle: (slug: string, checked: boolean) => void;
}

export default function ManageExtensionsPanel({
  browserName,
  browserEnabled,
  extensionToggles,
  localLoadingMap,
  onToggle,
}: ManageExtensionsPanelProps) {
  return (
    <div className="flex flex-col gap-2">
      <div className="text-xs font-semibold uppercase tracking-widest opacity-60">
        Extensions for {browserName}
      </div>
      {!browserEnabled && (
        <p className="text-xs text-[var(--text-dim)]">
          Enable this browser to turn on and manage its extensions. Your extension choices are preserved.
        </p>
      )}
      {/* uBlock Origin isn't listed — it's always included. Toggling one off
          here re-applies hardening to any already-hardened browser so the
          removal takes effect immediately, not on the next harden. */}
      <div className="grid grid-cols-2 gap-2">
        {BROWSER_EXTENSION_TOGGLES.map((ext) => (
          <UniversalToggle
            key={ext.slug}
            label={ext.name}
            description={ext.description}
            icon={ext.icon}
            iconImage={ext.iconImage}
            checked={browserEnabled && isBrowserExtensionEnabled(extensionToggles, browserName, ext.slug)}
            onChange={(checked) => onToggle(ext.slug, checked)}
            loading={localLoadingMap[`ext_${browserExtensionSettingKey(browserName, ext.slug)}`]}
            disabled={!browserEnabled || localLoadingMap[`ext_${browserExtensionSettingKey(browserName, ext.slug)}`]}
            size="compact"
          />
        ))}
      </div>
    </div>
  );
}
