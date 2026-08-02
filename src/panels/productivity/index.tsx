// ══════════════════════════════════════════════════════════════════════════
// Productivity Panel — ActivityWatch via EmbeddedWebView (native WebView2)
// ══════════════════════════════════════════════════════════════════════════
// Uses EmbeddedWebView (components/shared/EmbeddedWebView.tsx) so we bypass
// iframe CSP restrictions from ActivityWatch localhost server.
//
// HOSTNAME: Pulled from systemInfo.hostname (COMPUTERNAME) at runtime.
// ActivityWatch uses $env:COMPUTERNAME as its bucket hostname, so this is
// the correct value across all machines. Falls back to "localhost" while
// systemInfo is loading (AW also accepts this for the status page).
//
// GROUP: "productivity" — webview IDs match view IDs: activity, timeline, search.
// key=`${activeView}-${hostname}` on EmbeddedWebView forces re-mount when
// either the tab OR the resolved hostname changes.
// ══════════════════════════════════════════════════════════════════════════

import { useState, useEffect } from 'react';
import { H5, Text, Icon } from "@/components/ui/bp";
import EmbeddedWebView from '../../components/shared/EmbeddedWebView';
import { useAppState } from '../../context/AppContext';
import { invoke } from "@tauri-apps/api/core";
import { open } from '@tauri-apps/plugin-shell';
import './index.css';

// CSS injected via initialization_script to strip ActivityWatch chrome
const AW_HIDE_CSS = `
.navbar-expand-lg.navbar-light.aw-navbar.navbar { display: none !important; }
div.container > .mb-2 { display: none !important; }
.my-2.float-md-left.float-none { display: none !important; }
.my-2.float-md-right.float-none { display: none !important; }
footer, .footer, .aw-footer { display: none !important; }
`.trim();

type ProductivityView = 'activity' | 'timeline' | 'search';

interface ViewConfig {
  id: ProductivityView;
  label: string;
  icon: string;
  buildUrl: (hostname: string) => string;
}

const VIEW_CONFIGS: ViewConfig[] = [
  {
    id: 'activity',
    label: 'Activity',
    icon: '📊',
    // AW uses COMPUTERNAME as bucket hostname — must match exactly
    buildUrl: (h) => `http://localhost:5600/#/activity/${h}/view/`,
  },
  {
    id: 'timeline',
    label: 'Timeline',
    icon: '🕐',
    buildUrl: () => 'http://localhost:5600/#/timeline',
  },
  {
    id: 'search',
    label: 'Search',
    icon: '🔍',
    buildUrl: () => 'http://localhost:5600/#/search',
  },
];

export default function ProductivityPanel() {
  const [activeView, setActiveView] = useState<ProductivityView>('activity');
  const { systemInfo } = useAppState();

  // On unmount, make sure we hide the webview to prevent overlaps
  useEffect(() => {
    return () => {
      invoke('hide_all_server_apps', { group: 'productivity' }).catch(console.error);
    };
  }, []);

  // Use real hostname once loaded; "localhost" lets AW render the home/status
  // page while systemInfo is still being fetched on startup.
  const hostname = systemInfo?.hostname || 'localhost';

  const activeConfig = VIEW_CONFIGS.find(v => v.id === activeView)!;
  const activeUrl = activeConfig.buildUrl(hostname);

  return (
    <div className="panel-container productivity-panel">
      <div className="productivity-header">
        <div>
          <H5 className="header-title">Productivity</H5>
          <Text className="header-subtext">Data stays on this device. Never uploaded.</Text>
        </div>

        {/* Browser & IDE Extension links */}
        <div className="productivity-watchers">
          <span className="watchers-label">WATCHERS</span>
          <div className="watcher-links">
            <button type="button" onClick={() => open('https://chromewebstore.google.com/detail/activitywatch-web-watcher/nglaklhklhcoonedhgnpgddginnjdadi')} className="watcher-badge" title="Chrome Extension">
              <Icon icon="globe-network" size={12} /> Chrome
            </button>
            <button type="button" onClick={() => open('https://addons.mozilla.org/en-US/firefox/addon/aw-watcher-web/')} className="watcher-badge" title="Firefox Add-on">
              <Icon icon="globe-network" size={12} /> Firefox
            </button>
            <button type="button" onClick={() => open('https://marketplace.visualstudio.com/items?itemName=activitywatch.aw-watcher-vscode')} className="watcher-badge" title="VS Code Extension">
              <Icon icon="code" size={12} /> VS Code
            </button>
          </div>
        </div>
      </div>

      {/* Tab bar — spatially above native webview (no z-order conflict) */}
      <div className="productivity-view-tabs">
        {VIEW_CONFIGS.map((v) => (
          <button
            key={v.id}
            type="button"
            aria-pressed={activeView === v.id}
            className={`productivity-view-tab ${activeView === v.id ? 'active' : ''}`}
            onClick={() => activeView !== v.id && setActiveView(v.id)}
          >
            <span className="tab-icon">{v.icon}</span>
            <span className="tab-label">{v.label}</span>
          </button>
        ))}
      </div>

      {/* key includes hostname so webview re-mounts once real hostname resolves */}
      <EmbeddedWebView
        key={`${activeView}-${hostname}`}
        group="productivity"
        id={activeView}
        url={activeUrl}
        customCss={AW_HIDE_CSS}
        label={activeConfig.label}
        style={{
          borderRadius: 12,
          border: '1px solid var(--color-border)',
          background: 'var(--color-bg-primary)',
        }}
      />
    </div>
  );
}
