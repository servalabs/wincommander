// Category definitions for the cleanup inline card grid.
// Each entry defines how to fetch, preview, and clear a cleanup trace category.

export interface CleanupCategory {
  id: string;
  label: string;
  description: string;
  icon: string;         // BlueprintJS icon name
  color: string;        // accent color for the card
  severity: 'warning' | 'danger';
  group: 'standard' | 'deep-dfir' | 'action-only';
  /** Key into the useBackend hook to fetch data */
  getDataKey: string;
  /** Key into the useBackend hook to clear data */
  clearDataKey: string;
  /** Extract count + normalized item labels from raw backend response.
   * Cards slice this list for preview; TraceDetailDialog shows the full list. */
  extractPreview: (data: any) => { count: number; items: string[] };
  /** If true, this is a "run" action with no viewable data */
  actionOnly?: boolean;
  /** Confirm message before clearing (null = no confirm) */
  confirmMessage?: string;
  /** Whether this category supports recurring auto-erase scheduling. */
  schedulable?: boolean;
  /** User-impact tier used to group cleanup cards. */
  usabilityTier?: CleanupUsabilityTier;
  /** Backend scheduler category id when it differs from the UI card id. */
  schedulerCategoryId?: string;
  /**
   * Minimum allowed interval (minutes) for auto-erase. DFIR clearers are
   * heavy (Search Index rebuild, Amcache rewrite) and shouldn't run more
   * than hourly even if the user picks Custom.
   */
  minIntervalMinutes?: number;
  /**
   * Run the scheduled task as NT AUTHORITY\SYSTEM (vs the current user
   * via S4U). Required when the clearer touches TrustedInstaller-ACL'd
   * paths or needs the SYSTEM-only ClearLog right. See
   * commander-anticleanup-protected-keys memory for the list of paths
   * that demand SYSTEM context.
   */
  schedulerRunAsSystem?: boolean;
  /**
   * If true, this category stores data per Windows user account (HKCU,
   * %APPDATA%, %LOCALAPPDATA%). The scope selector is shown in the UI so
   * the operator can choose Current User / All Users / Select Users.
   * Cleared via Invoke-CleanupClearAllUsers when scope != 'current'.
   */
  scopeAware?: boolean;
  /**
   * If true, this category operates at the system level and already
   * affects all users regardless of who runs it (e.g. event logs,
   * prefetch, USB history). The scope selector is hidden for these.
   */
  systemWide?: boolean;
  /**
   * Short note shown as a tooltip on the card explaining why data may
   * reappear immediately after clearing (OS-managed stores that Windows
   * continuously writes to). Omit for categories where cleared data
   * stays gone until the user does something specific.
   */
  regeneratesNote?: string;
}

export type CleanupUsabilityTier =
  | 'low-impact'
  | 'history-cache'
  | 'rebuilds-apps-connectivity'
  | 'data-accounts-recovery';

export const CLEANUP_USABILITY_TIERS: ReadonlyArray<{
  id: CleanupUsabilityTier;
  label: string;
  description: string;
  color: string;
}> = [
  { id: 'low-impact', label: 'Low impact', description: 'Clears diagnostics and system records without removing saved setup or personal files.', color: 'var(--color-success)' },
  { id: 'history-cache', label: 'History & cache', description: 'Removes recent items and local caches that make apps more convenient to use.', color: 'var(--color-info)' },
  { id: 'rebuilds-apps-connectivity', label: 'Rebuilds apps or connectivity', description: 'Can make Windows rebuild data, slow a feature briefly, or refresh a connection.', color: 'var(--color-warning)' },
  { id: 'data-accounts-recovery', label: 'Data, accounts & recovery', description: 'Can delete files, saved sign-ins, profiles, backups, or development environments.', color: 'var(--color-danger)' },
];

// ── Standard Trace Auditing Categories ────────────────────────────────

export const STANDARD_CATEGORIES: CleanupCategory[] = [
  {
    id: 'shellBags',
    label: 'ShellBags',
    description: 'Folder access history',
    icon: 'folder-open',
    color: '#f59e0b',
    severity: 'warning',
    group: 'standard',
    scopeAware: true,
    getDataKey: 'getShellBags',
    clearDataKey: 'clearShellBags',
    extractPreview: (data) => {
      const entries = Array.isArray(data?.entries) ? data.entries : [];
      return {
        count: entries.length,
        items: entries.map((e: any) => e.path || e.name || '—'),
      };
    },
  },
  {
    id: 'usbHistory',
    label: 'USB History',
    description: 'Connected devices',
    icon: 'drive-time',
    color: '#3b82f6',
    severity: 'warning',
    group: 'standard',
    systemWide: true,
    getDataKey: 'getUsbHistory',
    clearDataKey: 'clearUsbHistory',
    extractPreview: (data) => {
      const devices = Array.isArray(data?.devices) ? data.devices : [];
      return {
        count: devices.length,
        items: devices.map((d: any) => d.friendlyName || d.description || d.deviceId || '—'),
      };
    },
  },
  {
    id: 'recycleBin',
    label: 'Recycle Bin Records',
    description: 'Deleted-item records on fixed and removable drives',
    icon: 'trash',
    color: '#0ea5e9',
    severity: 'warning',
    group: 'standard',
    systemWide: true,
    getDataKey: 'getRecycleBinInfo',
    clearDataKey: 'clearRecycleBinMetadata',
    confirmMessage: 'Empty the Recycle Bin and remove deleted-item records from fixed and removable drives (original path / delete time)? This cannot be undone.',
    extractPreview: (data) => {
      const items = Array.isArray(data?.items) ? data.items : [];
      return {
        count: data?.total ?? items.length,
        items: items.map((e: any) =>
          e.deletedTime ? `${e.originalPath || '—'} (deleted ${e.deletedTime})` : (e.originalPath || '—')
        ),
      };
    },
  },
  {
    id: 'dnsCache',
    label: 'DNS Cache',
    description: 'Resolver cache entries',
    icon: 'globe-network',
    color: '#8b5cf6',
    severity: 'warning',
    group: 'standard',
    systemWide: true,
    getDataKey: 'getDnsCacheEntries',
    clearDataKey: 'flushDnsCache',
    extractPreview: (data) => {
      const entries = Array.isArray(data?.entries) ? data.entries : [];
      return {
        count: entries.length,
        items: entries.map((e: any) => e.name || e.entry || '—'),
      };
    },
  },
  {
    id: 'clipboardHistory',
    label: 'Clipboard History',
    description: 'Windows clipboard & cloud sync history status',
    icon: 'clipboard',
    color: '#4b5563',
    severity: 'warning',
    group: 'standard',
    scopeAware: true,
    getDataKey: 'getClipboardHistoryStatus',
    clearDataKey: 'clearClipboard',
    confirmMessage: 'Clear clipboard history and cloud sync cache now?',
    extractPreview: (data) => {
      const status = data || {};
      const histDisabled = !!status.clipboardHistoryDisabled;
      const cloudDisabled = !!status.cloudClipboardDisabled;
      const rawItems: Array<{ type: string; preview: string; charCount: number }> = Array.isArray(status.historyItems) ? status.historyItems : [];

      if (rawItems.length > 0) {
        return {
          count: rawItems.length,
          items: rawItems.map((it) =>
            it.type === 'text' && it.charCount > 0
              ? `${it.preview}${it.charCount > 80 ? ` (${it.charCount} chars)` : ''}`
              : it.preview || '[Other content]'
          ),
        };
      }

      // History disabled or empty — show status flags only
      return {
        count: histDisabled && cloudDisabled ? 0 : 1,
        items: [
          `History: ${histDisabled ? 'Disabled' : 'Enabled'}`,
          `Cloud sync: ${cloudDisabled ? 'Disabled' : 'Enabled'}`,
        ],
      };
    },
  },
  {
    id: 'execCache',
    label: 'Execution Audit',
    description: 'ShimCache, UserAssist traces',
    icon: 'code',
    color: '#ef4444',
    severity: 'warning',
    group: 'standard',
    scopeAware: true,
    regeneratesNote: 'UserAssist refills as you use apps; ShimCache rebuilds on reboot. MuiCache reappears as programs run.',
    getDataKey: 'getExecutionCache',
    clearDataKey: 'clearExecutionCache',
    extractPreview: (data) => {
      const entries = Array.isArray(data?.entries) ? data.entries : [];
      return {
        count: entries.length,
        items: entries.map((e: any) => {
          const p = e.path || e.name || '—';
          const parts = p.replace(/\\/g, '/').split('/');
          return parts[parts.length - 1] || p;
        }),
      };
    },
  },
  {
    id: 'wlanProfiles',
    label: 'Wi-Fi Profiles',
    description: 'Saved network profiles',
    icon: 'feed',
    color: '#10b981',
    severity: 'warning',
    group: 'standard',
    systemWide: true,
    getDataKey: 'getWlanProfiles',
    clearDataKey: 'removeWlanProfile',
    extractPreview: (data) => {
      const profiles = Array.isArray(data?.profiles) ? data.profiles : [];
      return {
        count: profiles.length,
        items: profiles.map((p: any) => p.name || '—'),
      };
    },
  },
  // {
  //   id: 'btDevices',
  //   label: 'Bluetooth',
  //   description: 'Paired device history',
  //   icon: 'satellite',
  //   color: '#6366f1',
  //   severity: 'warning',
  //   group: 'standard',
  //   getDataKey: 'getBluetoothDevices',
  //   clearDataKey: 'clearBluetoothHistory',
  //   extractPreview: (data) => {
  //     const devices = Array.isArray(data?.devices) ? data.devices : [];
  //     return {
  //       count: devices.length,
  //       items: devices.map((d: any) => d.name || 'Unknown'),
  //     };
  //   },
  // },
  {
    id: 'netDrives',
    label: 'Net Drives',
    description: 'Mapped network shares',
    icon: 'cloud',
    color: '#0ea5e9',
    severity: 'warning',
    group: 'standard',
    scopeAware: true,
    getDataKey: 'getNetworkDrives',
    clearDataKey: 'clearNetworkDrives',
    extractPreview: (data) => {
      const drives = Array.isArray(data?.drives) ? data.drives : [];
      return {
        count: drives.length,
        items: drives.map((d: any) => `${d.localName || '?'} → ${d.remoteName || '?'}`),
      };
    },
  },
  {
    id: 'eventLogs',
    label: 'Event Logs',
    description: 'Windows event log volumes',
    icon: 'timeline-events',
    color: '#a855f7',
    severity: 'warning',
    group: 'standard',
    systemWide: true,
    regeneratesNote: 'Windows writes new events continuously — logs refill within seconds of clearing.',
    getDataKey: 'getEventLogSummary',
    clearDataKey: 'clearEventLogs',
    extractPreview: (data) => {
      const logs = Array.isArray(data?.logs) ? data.logs : [];
      const totalEntries = logs.reduce((s: number, l: any) => s + (l.entries || l.count || 0), 0);
      return {
        count: totalEntries,
        items: logs.map((l: any) => `${l.logName || l.name}: ${l.entries || l.count || 0}`),
      };
    },
  },
  {
    id: 'psHistory',
    label: 'Command History',
    description: 'Terminal command log',
    icon: 'console',
    color: '#0284c7',
    severity: 'warning',
    group: 'standard',
    scopeAware: true,
    getDataKey: 'getPSHistory',
    clearDataKey: 'clearPowerShellHistory',
    extractPreview: (data) => {
      const entries = Array.isArray(data?.entries) ? data.entries : [];
      return {
        count: data?.fileTotal ?? entries.length,
        items: entries.map((e: any) => (e.command || e.line || '—').substring(0, 50)),
      };
    },
  },
  {
    id: 'recentFiles',
    label: 'Recent Files',
    description: 'Shell Recent folder links',
    icon: 'document',
    color: '#f97316',
    severity: 'warning',
    group: 'standard',
    scopeAware: true,
    regeneratesNote: 'New entries appear each time a file is opened — any application activity after clearing re-populates this.',
    getDataKey: 'getRecentFiles',
    clearDataKey: 'clearRecentFiles',
    extractPreview: (data) => {
      const entries = Array.isArray(data?.entries) ? data.entries : [];
      return {
        count: entries.length,
        items: entries.map((e: any) => e.name || e.fileName || '—'),
      };
    },
  },
  {
    id: 'rdpHistory',
    label: 'RDP History',
    description: 'Remote Desktop connections',
    icon: 'desktop',
    color: '#ec4899',
    severity: 'warning',
    group: 'standard',
    scopeAware: true,
    getDataKey: 'getRDPHistory',
    clearDataKey: 'clearRDPHistory',
    extractPreview: (data) => {
      const entries = Array.isArray(data?.entries) ? data.entries : [];
      return {
        count: entries.length,
        items: entries.map((e: any) => e.host || e.server || e.hostname || '—'),
      };
    },
  },
  {
    id: 'jumpLists',
    label: 'Jump Lists',
    description: 'App recent/pinned items',
    icon: 'list-detail-view',
    color: '#eab308',
    severity: 'warning',
    group: 'standard',
    scopeAware: true,
    getDataKey: 'getJumpLists',
    clearDataKey: 'clearJumpLists',
    extractPreview: (data) => {
      const entries = Array.isArray(data?.entries) ? data.entries : [];
      return {
        count: entries.length,
        items: entries.map((e: any) => e.appName || e.name || '—'),
      };
    },
  },
  {
    id: 'connectivityHistory',
    label: 'Connectivity History',
    description: 'Known network profiles and NetworkList signatures',
    icon: 'network',
    color: '#38bdf8',
    severity: 'warning',
    group: 'standard',
    systemWide: true,
    regeneratesNote: 'Windows re-creates network profiles for any currently active connection immediately after clearing.',
    getDataKey: 'getConnectivityHistory',
    clearDataKey: 'clearConnectivityHistory',
    extractPreview: (data) => {
      const entries = Array.isArray(data?.entries) ? data.entries : [];
      return {
        count: data?.total ?? entries.length,
        // Show source type alongside name so user understands these are OS-maintained profiles
        items: entries.map((e: any) => {
          const name = e.name || e.description || e.dnsSuffix || e.key || '—';
          return e.source ? `[${e.source}] ${name}` : name;
        }),
      };
    },
  },
  {
    id: 'browserFootprints',
    label: 'Browser Audit',
    description: 'Footprint sizes per browser',
    icon: 'globe',
    color: '#06b6d4',
    severity: 'warning',
    group: 'standard',
    scopeAware: true,
    getDataKey: 'getBrowserFootprints',
    clearDataKey: 'clearBrowserFootprints',
    extractPreview: (data) => {
      const browsers = Array.isArray(data?.browsers) ? data.browsers : [];
      return {
        count: browsers.length,
        items: browsers.map((b: any) => {
          const name = b.browser || b.name || '?';
          const sizeKB = b.totalSizeKB ?? 0;
          const sizeMB = (sizeKB / 1024).toFixed(1);
          return `${name}: ${sizeMB}MB`;
        }),
      };
    },
  },
  {
    id: 'prefetchFiles',
    label: 'Prefetch Files',
    description: 'App execution history (.pf)',
    icon: 'flash',
    color: '#eab308',
    severity: 'warning',
    group: 'standard',
    systemWide: true,
    getDataKey: 'getPrefetchFiles',
    clearDataKey: 'clearPrefetch',
    extractPreview: (data) => {
      const entries = Array.isArray(data?.entries) ? data.entries : [];
      return {
        count: entries.length,
        items: entries.map((e: any) => e.name || e.exeName || '—'),
      };
    },
  },
  {
    id: 'shadowCopies',
    label: 'Shadow Copies',
    description: 'VSS snapshots inventory',
    icon: 'duplicate',
    color: '#64748b',
    severity: 'warning',
    group: 'standard',
    systemWide: true,
    getDataKey: 'getShadowCopies',
    clearDataKey: 'clearShadowCopies',
    confirmMessage: 'Delete ALL shadow copies? This cannot be undone.',
    extractPreview: (data) => {
      const copies = Array.isArray(data?.copies) ? data.copies : [];
      return {
        count: copies.length,
        items: copies.map((c: any) => c.creationTime || c.id || '—'),
      };
    },
  },
  {
    id: 'ntfsJournals',
    label: 'NTFS Journals',
    description: 'USN journal per drive',
    icon: 'database',
    color: '#78716c',
    severity: 'warning',
    group: 'standard',
    systemWide: true,
    regeneratesNote: 'The NTFS USN journal is a kernel requirement — Windows re-creates it immediately. Clearing resets history to zero bytes.',
    getDataKey: 'getNTFSJournals',
    clearDataKey: 'clearNTFSJournals',
    extractPreview: (data) => {
      const journals = Array.isArray(data?.journals) ? data.journals : [];
      const active = journals.filter((j: any) => j.present);
      // Note: count stays the same after clearing (journals are immediately recreated by
      // the kernel). The items show drive + max size so the user can see the journal
      // was reset (journalId changes, history resets to zero accumulated bytes).
      return {
        count: active.length,
        items: active.map((j: any) => `${j.drive || '?'}: journal ${j.journalId || 'active'}${j.maxSize ? ` (max ${j.maxSize})` : ''}`),
      };
    },
  },
];

// ── Deep Cleanup Categories ───────────────────────────────────────────

export const DEEP_DFIR_CATEGORIES: CleanupCategory[] = [
  {
    id: 'amcache',
    label: 'App Launch Cache',
    description: 'Windows app launch records',
    icon: 'history',
    color: '#dc2626',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    regeneratesNote: 'Windows repopulates Amcache as programs run — new entries appear within minutes of clearing.',
    getDataKey: 'getAmcacheEntries',
    clearDataKey: 'clearAmcache',
    confirmMessage: 'Clear app launch records?',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.entries || []).map((e: any) =>
        `${e.category || 'Category'}: ${e.count || 0} entries`
      ),
    }),
  },
  {
    id: 'ntUserTraces',
    label: 'User Activity Cache',
    description: 'Run box, typed paths, file picker history',
    icon: 'key',
    color: '#b91c1c',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    regeneratesNote: 'Entries are added each time you use the Run dialog, address bar, or file picker — normal use re-creates this quickly.',
    getDataKey: 'getNTUserTraces',
    clearDataKey: 'clearNTUserTraces',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.sections || []).map((s: any) =>
        `${s.name}: ${s.count} entries`
      ),
    }),
  },
  {
    id: 'notepadState',
    label: 'Notepad State',
    description: 'Unsaved tabs and temporary editor state',
    icon: 'edit',
    color: '#ea580c',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    getDataKey: 'getNotepadStateFiles',
    clearDataKey: 'clearNotepadState',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'pcaDatabase',
    label: 'Compatibility Cache',
    description: 'Program compatibility records',
    icon: 'cog',
    color: '#c2410c',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    regeneratesNote: 'PcaSvc recreates compatibility records as programs are launched.',
    getDataKey: 'getPCAInfo',
    clearDataKey: 'clearPCADatabase',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'crashDumps',
    label: 'Crash Dumps',
    description: 'Windows crash reports and dump files',
    icon: 'error',
    color: '#991b1b',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    getDataKey: 'getCrashDumpList',
    clearDataKey: 'invokeCrashDumpErase',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.dumps || []).map((d: any) => `${d.source}: ${d.name}`),
    }),
  },
  {
    id: 'searchIndex',
    label: 'Search Index',
    description: 'Windows indexed content cache',
    icon: 'search',
    color: '#7c3aed',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    regeneratesNote: 'WSearch rebuilds the index automatically — files reappear as the service re-indexes. Full rebuild may take minutes to hours.',
    getDataKey: 'getSearchIndexInfo',
    clearDataKey: 'clearSearchIndex',
    confirmMessage: 'Clear Windows Search index? Search will rebuild after (may take minutes).',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => `${f.label || f.name}: ${f.sizeKB}KB`),
    }),
  },
  {
    id: 'printSpooler',
    label: 'Print Spooler',
    description: 'Temporary print queue files',
    icon: 'print',
    color: '#475569',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    getDataKey: 'getPrintSpoolerInfo',
    clearDataKey: 'clearPrintSpooler',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'srumData',
    label: 'Resource Usage History',
    description: 'Per-app CPU, network, and energy usage history (live process snapshot + SRUDB.dat status)',
    icon: 'dashboard',
    color: '#ec4899',
    severity: 'warning',
    group: 'deep-dfir',
    systemWide: true,
    regeneratesNote: 'SRUDB.dat is locked while Windows runs — viewer shows live processes as proxy. The database is wiped on clear; DPS recreates it within seconds.',
    getDataKey: 'getSRUMData',
    clearDataKey: 'clearSRUM',
    extractPreview: (data) => {
      const entries = Array.isArray(data?.entries) ? data.entries : [];
      const srumSizeMb = data?.srumSizeMb;
      // Show database status as first item so it's visible immediately after clear.
      const dbLine = srumSizeMb != null
        ? `SRUDB.dat: ${srumSizeMb} MB`
        : 'SRUDB.dat: cleared (not present)';
      const processLines = entries.map((e: any) =>
        e.name || e.appId || e.exeInfo || e.path?.split(/[\\/]/).pop() || '—'
      );
      return {
        // Count based on SRUDB.dat existence — not the always-present process list.
        count: srumSizeMb != null ? entries.length : 0,
        items: [dbLine, ...processLines],
      };
    },
  },
  {
    id: 'walFiles',
    label: 'Temp Database Files',
    description: 'Leftover database fragments in app data folders',
    icon: 'th',
    color: '#ca8a04',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    getDataKey: 'getSQLiteWALList',
    clearDataKey: 'invokeSQLiteWALKiller',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'recallDb',
    label: 'Activity Timeline',
    description: 'Windows timeline and activity databases',
    icon: 'eye-open',
    color: '#be185d',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    regeneratesNote: 'If Windows Recall or Timeline is enabled, the OS recreates snapshot databases continuously in the background.',
    getDataKey: 'getRecallDatabaseInfo',
    clearDataKey: 'clearRecallDatabase',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.databases || []).map((d: any) => `${d.source}: ${d.name}`),
    }),
  },
  {
    id: 'webCache',
    label: 'Web Cache Database',
    description: 'Edge/WinINET browsing history, cookies and cached responses (WebCacheV01.dat)',
    icon: 'globe-network',
    color: '#7c3aed',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    regeneratesNote: 'Windows and Edge rebuild this store as you browse.',
    getDataKey: 'getWebCacheInfo',
    clearDataKey: 'clearWebCache',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || f.path || '—'),
    }),
  },
  {
    id: 'thumbnailDb',
    label: 'Thumbnail & Icon Cache',
    description: 'Explorer thumbnail and icon databases (thumbcache_*.db, iconcache_*.db)',
    icon: 'media',
    color: '#db2777',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    regeneratesNote: 'Explorer rebuilds thumbnails and icons as you browse folders.',
    getDataKey: 'getThumbnailCacheInfo',
    clearDataKey: 'clearThumbnailCache',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'notificationDb',
    label: 'Notification History',
    description: 'Action Center toast and app notification database (wpndatabase.db)',
    icon: 'notifications',
    color: '#c026d3',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    regeneratesNote: 'New notifications repopulate the store immediately.',
    getDataKey: 'getNotificationDbInfo',
    clearDataKey: 'clearNotificationDb',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'branchCache',
    label: 'Peer Distribution Cache',
    description: 'Locally cached blocks of downloaded network content (BranchCache / PeerDist)',
    icon: 'data-connection',
    color: '#9333ea',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    regeneratesNote: 'Repopulates when BranchCache-enabled downloads occur.',
    getDataKey: 'getBranchCacheInfo',
    clearDataKey: 'clearBranchCache',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'eventTranscript',
    label: 'Diagnostics Timeline',
    description: 'Windows telemetry activity database (EventTranscript.db)',
    icon: 'timeline-events',
    color: '#e11d48',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    regeneratesNote: 'The diagnostic data / telemetry service repopulates EventTranscript.db continuously while diagnostic data collection is enabled.',
    getDataKey: 'getEventTranscriptInfo',
    clearDataKey: 'clearEventTranscript',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || f.path || '—'),
    }),
  },
  {
    id: 'activitiesTimeline',
    label: 'Timeline Cache',
    description: 'Per-user Windows Timeline database (ActivitiesCache.db)',
    icon: 'list-detail-view',
    color: '#a21caf',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    regeneratesNote: 'The Windows Timeline / Activity Feed service repopulates ActivitiesCache.db continuously while enabled.',
    getDataKey: 'getActivitiesTimelineInfo',
    clearDataKey: 'clearActivitiesTimeline',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || f.path || '—'),
    }),
  },
  {
    id: 'rdpBitmapCache',
    label: 'Remote Session Cache',
    description: 'Cached bitmap tiles from Remote Desktop sessions (reconstructable screenshots)',
    icon: 'desktop',
    color: '#be123c',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    regeneratesNote: 'Windows rebuilds the bitmap cache from scratch the next time an RDP session connects.',
    getDataKey: 'getRdpBitmapCacheInfo',
    clearDataKey: 'clearRdpBitmapCache',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || f.path || '—'),
    }),
  },
  {
    id: 'servicingLogs',
    label: 'Servicing Logs',
    description: 'Component install/update history (CBS / DISM logs)',
    icon: 'cog',
    color: '#0f766e',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    regeneratesNote: 'CBS/DISM logs are rewritten on every subsequent servicing operation (Windows Update, DISM, component install).',
    getDataKey: 'getServicingLogsInfo',
    clearDataKey: 'clearServicingLogs',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || f.path || '—'),
    }),
  },
  {
    id: 'deviceInstallLogs',
    label: 'Device Install Logs',
    description: 'PnP and USB device install history (setupapi logs)',
    icon: 'drive-time',
    color: '#1d4ed8',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    regeneratesNote: 'setupapi logs are recreated the next time a PnP or USB device is installed or reconnected.',
    getDataKey: 'getDeviceInstallLogsInfo',
    clearDataKey: 'clearDeviceInstallLogs',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || f.path || '—'),
    }),
  },
  {
    id: 'usageTraceLogs',
    label: 'Usage Trace Logs',
    description: 'Power/usage ETW trace logs (SleepStudy, WDI, WMI .etl)',
    icon: 'dashboard',
    color: '#7c2d12',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    regeneratesNote: 'SleepStudy/WDI/WMI ETW traces are regenerated automatically by the diagnostic tracing services on their normal schedule.',
    getDataKey: 'getUsageTraceLogsInfo',
    clearDataKey: 'clearUsageTraceLogs',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || f.path || '—'),
    }),
  },
  {
    id: 'defenderHistory',
    label: 'Protection History',
    description: 'Microsoft Defender scan/detection history and command log',
    icon: 'diagnosis',
    color: '#15803d',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    regeneratesNote: 'Microsoft Defender writes new entries to Protection History on every scheduled or manual scan.',
    getDataKey: 'getDefenderHistoryInfo',
    clearDataKey: 'clearDefenderHistory',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || data?.entries || []).map((f: any) => f.name || f.path || '—'),
    }),
  },
  {
    id: 'wslData',
    label: 'WSL Data',
    description: 'Linux distribution storage and local activity files',
    icon: 'database',
    color: '#2563eb',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getWSLDataInfo',
    clearDataKey: 'clearWSLData',
    confirmMessage: 'Clear WSL data? This can remove Linux distribution storage and local files. WSL distributions may need to be restored or recreated. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'dockerDesktopData',
    label: 'Docker Desktop Data',
    description: 'Docker Desktop storage, logs, and local cache files',
    icon: 'cloud',
    color: '#0891b2',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getDockerDesktopDataInfo',
    clearDataKey: 'clearDockerDesktopData',
    confirmMessage: 'Clear Docker Desktop data? This can remove local images, containers, volumes, logs, and cache files. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'virtualMachineArtifacts',
    label: 'Virtual Machine Artifacts',
    description: 'VM snapshots, logs, and local configuration files',
    icon: 'desktop',
    color: '#7c3aed',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getVirtualMachineArtifactsInfo',
    clearDataKey: 'clearVirtualMachineArtifacts',
    confirmMessage: 'Clear virtual machine artifacts? This can remove VM snapshots, logs, and local configuration files. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'developerCaches',
    label: 'Developer Tool Caches',
    description: 'Package manager and developer tool cache files',
    icon: 'code',
    color: '#c2410c',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getDeveloperCachesInfo',
    clearDataKey: 'clearDeveloperCaches',
    confirmMessage: 'Clear developer tool caches? This can remove package caches and local credential or configuration files used by developer tools. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'credentialManager',
    label: 'Saved Credentials',
    description: 'Stored Windows and application credential entries',
    icon: 'key',
    color: '#b45309',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getCredentialManagerInfo',
    clearDataKey: 'clearCredentialManager',
    confirmMessage: 'Clear saved credentials? This removes stored sign-in credentials for Windows, network resources, and applications. You may need to sign in again. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'networkWizardHistory',
    label: 'Network Wizard History',
    description: 'Network connection setup history entries',
    icon: 'network',
    color: '#0284c7',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getNetworkWizardHistoryInfo',
    clearDataKey: 'clearNetworkWizardHistory',
    confirmMessage: 'Clear Network Connection Wizard history? This removes recorded network setup entries. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'werHistory',
    label: 'Windows Error Reporting History',
    description: 'Windows Error Reporting consent and exclusion entries',
    icon: 'error',
    color: '#be123c',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    schedulable: false,
    getDataKey: 'getWERHistoryInfo',
    clearDataKey: 'clearWERHistory',
    confirmMessage: 'Clear Windows Error Reporting history? This removes saved consent and exclusion entries. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'inactiveUserProtectionMetadata',
    label: 'Inactive User Protection Metadata',
    description: 'Protection metadata for OTHER/INACTIVE user profiles only',
    icon: 'key',
    color: '#991b1b',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    schedulable: false,
    getDataKey: 'getInactiveUserProtectionMetadataInfo',
    clearDataKey: 'clearInactiveUserProtectionMetadata',
    confirmMessage: 'Clear protection metadata for OTHER/INACTIVE user profiles only? Deletion permanently breaks decryption for data protected by those profiles. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'stickyNotes',
    label: 'Sticky Notes',
    description: 'Sticky Notes database and local state files',
    icon: 'edit',
    color: '#ca8a04',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getStickyNotesInfo',
    clearDataKey: 'clearStickyNotes',
    confirmMessage: 'Clear Sticky Notes data? This deletes the local note database and unsynced notes may be lost. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'oneDriveMetadata',
    label: 'OneDrive Sync Metadata',
    description: 'OneDrive local sync-engine metadata files',
    icon: 'cloud',
    color: '#2563eb',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getOneDriveMetadataInfo',
    clearDataKey: 'clearOneDriveMetadata',
    confirmMessage: 'Clear OneDrive sync metadata? This removes local sync state and may require OneDrive to rebuild or sign in again. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'spotlightCache',
    label: 'Windows Spotlight Cache',
    description: 'Cached Windows Spotlight lock-screen images',
    icon: 'media',
    color: '#7c3aed',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getSpotlightCacheInfo',
    clearDataKey: 'clearSpotlightCache',
    confirmMessage: 'Clear the Windows Spotlight cache? This removes cached lock-screen images. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'fontCache',
    label: 'Font Cache',
    description: 'Windows and user font cache files',
    icon: 'document',
    color: '#475569',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    schedulable: false,
    getDataKey: 'getFontCacheInfo',
    clearDataKey: 'clearFontCache',
    confirmMessage: 'Clear the font cache? This removes Windows and user font cache files; fonts may rebuild and some applications may need to restart. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'legacyIconCache',
    label: 'Legacy Icon Cache',
    description: 'Legacy Explorer IconCache.db file',
    icon: 'media',
    color: '#db2777',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getLegacyIconCacheInfo',
    clearDataKey: 'clearLegacyIconCache',
    confirmMessage: 'Clear the legacy icon cache? This removes the legacy IconCache.db file and Explorer rebuilds it. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'gameCaptures',
    label: 'Game Captures',
    description: 'Recorded game clips and screenshots',
    icon: 'media',
    color: '#be123c',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getGameCapturesInfo',
    clearDataKey: 'clearGameCaptures',
    confirmMessage: 'Clear game captures? This permanently deletes recorded game clips and screenshots from local capture folders. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'photosCache',
    label: 'Photos Cache',
    description: 'Windows Photos cached data and thumbnails',
    icon: 'media',
    color: '#9333ea',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getPhotosCacheInfo',
    clearDataKey: 'clearPhotosCache',
    confirmMessage: 'Clear the Photos cache? This removes local Windows Photos cached data and thumbnails. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'xboxCache',
    label: 'Xbox Cache',
    description: 'Xbox app and Gaming Services cache data',
    icon: 'dashboard',
    color: '#15803d',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getXboxCacheInfo',
    clearDataKey: 'clearXboxCache',
    confirmMessage: 'Clear the Xbox cache? This removes local Xbox app and Gaming Services cache data. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'communicationCaches',
    label: 'Communication App Caches',
    description: 'Local chat, meeting, and attachment cache files',
    icon: 'notifications',
    color: '#2563eb',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getCommunicationCachesInfo',
    clearDataKey: 'clearCommunicationCaches',
    confirmMessage: 'Clear communication app caches? This removes locally stored chat, meeting, and attachment cache files. Apps may need to download content again. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'editorHistory',
    label: 'Editor History',
    description: 'Recent workspace and local editor history files',
    icon: 'edit',
    color: '#7c3aed',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getEditorHistoryInfo',
    clearDataKey: 'clearEditorHistory',
    confirmMessage: 'Clear editor history? This removes recent workspace records and local editor history. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'gitActivity',
    label: 'Git Activity',
    description: 'Git credential cache and repository activity records',
    icon: 'code',
    color: '#c2410c',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getGitActivityInfo',
    clearDataKey: 'clearGitActivity',
    confirmMessage: 'Clear Git activity data? This removes cached Git credentials and repository activity records. You may need to authenticate again. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'sshState',
    label: 'SSH State',
    description: 'SSH known-host records and agent state',
    icon: 'key',
    color: '#15803d',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getSSHStateInfo',
    clearDataKey: 'clearSSHState',
    confirmMessage: 'Clear SSH state? This removes known-host records and loaded agent identities. You may need to verify hosts or reload keys again. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'remoteAccessLogs',
    label: 'Remote Access Logs',
    description: 'Remote-access connection and session log files',
    icon: 'desktop',
    color: '#be123c',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getRemoteAccessLogsInfo',
    clearDataKey: 'clearRemoteAccessLogs',
    confirmMessage: 'Clear remote-access logs? This permanently removes connection and session log files. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'passwordManagerCaches',
    label: 'Password Manager Caches',
    description: 'Local password-manager cache and session files',
    icon: 'key',
    color: '#991b1b',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getPasswordManagerCachesInfo',
    clearDataKey: 'clearPasswordManagerCaches',
    confirmMessage: 'Clear password-manager caches? This removes local cached and session data. You may need to sign in and unlock password-manager apps again. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'gameLauncherLogs',
    label: 'Game Launcher Logs',
    description: 'Game launcher activity and diagnostic log files',
    icon: 'dashboard',
    color: '#ea580c',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getGameLauncherLogsInfo',
    clearDataKey: 'clearGameLauncherLogs',
    confirmMessage: 'Clear game launcher logs? This permanently removes local activity and diagnostic log files. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'adobeRecent',
    label: 'Adobe Recent Files',
    description: 'Adobe Reader and Acrobat recent-file records',
    icon: 'document',
    color: '#dc2626',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getAdobeRecentInfo',
    clearDataKey: 'clearAdobeRecent',
    confirmMessage: 'Clear Adobe recent-file records? This removes recent document and signature-history entries. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'officeTempFiles',
    label: 'Office Temporary Files',
    description: 'Office temporary, lock, and autosave files',
    icon: 'document',
    color: '#0f766e',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getOfficeTempFilesInfo',
    clearDataKey: 'clearOfficeTempFiles',
    confirmMessage: 'Clear Office temporary files? This permanently deletes temporary document, lock, and autosave files. Unsaved work may be lost. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'firewallLog',
    label: 'Firewall Log',
    description: 'Windows Firewall packet log files',
    icon: 'diagnosis',
    color: '#b91c1c',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    schedulable: false,
    getDataKey: 'getFirewallLogInfo',
    clearDataKey: 'clearFirewallLog',
    confirmMessage: 'Clear the Windows Firewall log? This deletes recorded firewall packet log entries. Logging will resume afterward. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'neighborCache',
    label: 'Network Neighbor Cache',
    description: 'Current network neighbor and address cache entries',
    icon: 'network',
    color: '#0284c7',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    schedulable: false,
    getDataKey: 'getNeighborCacheInfo',
    clearDataKey: 'clearNeighborCache',
    confirmMessage: 'Clear the network neighbor cache? Active connections may briefly need to rediscover nearby network devices. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'netbiosCache',
    label: 'NetBIOS Cache',
    description: 'Cached NetBIOS name-to-address mappings',
    icon: 'network',
    color: '#2563eb',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    schedulable: false,
    getDataKey: 'getNetBIOSCacheInfo',
    clearDataKey: 'clearNetBIOSCache',
    confirmMessage: 'Clear the NetBIOS cache? Network name mappings will need to be resolved again. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'geolocationCache',
    label: 'Geolocation Cache',
    description: 'Cached network-based location data',
    icon: 'globe',
    color: '#7c3aed',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    schedulable: false,
    getDataKey: 'getGeolocationCacheInfo',
    clearDataKey: 'clearGeolocationCache',
    confirmMessage: 'Clear the geolocation cache? This removes cached network-based location data. Location services may rebuild it. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'vpnPhonebooks',
    label: 'VPN Phonebooks',
    description: 'Saved VPN connection profiles and phonebook files',
    icon: 'desktop',
    color: '#0f766e',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getVPNPhonebooksInfo',
    clearDataKey: 'clearVPNPhonebooks',
    confirmMessage: 'Clear VPN phonebooks? This removes saved VPN connection profiles and phonebook files. You may need to recreate those profiles. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'proxyCache',
    label: 'Proxy Cache',
    description: 'Cached proxy and PAC configuration data',
    icon: 'globe-network',
    color: '#0891b2',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getProxyCacheInfo',
    clearDataKey: 'clearProxyCache',
    confirmMessage: 'Clear the proxy cache? This removes cached proxy and PAC configuration data. Connection settings will need to refresh. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'cloudPlaceholders',
    label: 'Cloud Sync Placeholders',
    description: 'Residual cloud-sync placeholder and reparse-point files',
    icon: 'cloud',
    color: '#9333ea',
    severity: 'danger',
    group: 'deep-dfir',
    scopeAware: true,
    schedulable: false,
    getDataKey: 'getCloudPlaceholdersInfo',
    clearDataKey: 'clearCloudPlaceholders',
    confirmMessage: 'Clear cloud-sync placeholders? This permanently deletes leftover cloud placeholder and reparse-point files from previously synchronized folders. Files may no longer be accessible locally. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'bitsQueue',
    label: 'BITS Transfer Queue',
    description: 'Background Intelligent Transfer Service jobs and queue files',
    icon: 'data-connection',
    color: '#c2410c',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    schedulable: false,
    getDataKey: 'getBITSQueueInfo',
    clearDataKey: 'clearBITSQueue',
    confirmMessage: 'Clear the BITS transfer queue? This cancels pending background transfer jobs and removes queue history. Downloads or uploads may be interrupted. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
  {
    id: 'cellularHistory',
    label: 'Cellular Connection History',
    description: 'Cellular and mobile-hotspot connection history',
    icon: 'feed',
    color: '#ea580c',
    severity: 'danger',
    group: 'deep-dfir',
    systemWide: true,
    schedulable: false,
    getDataKey: 'getCellularHistoryInfo',
    clearDataKey: 'clearCellularHistory',
    confirmMessage: 'Clear cellular connection history? This removes saved cellular and mobile-hotspot history. Active connections may need to reconnect. This cannot be undone.',
    extractPreview: (data) => ({
      count: data?.total ?? 0,
      items: (data?.files || []).map((f: any) => f.name || '—'),
    }),
  },
];

// Action-only categories (no viewer data)
export const ACTION_CATEGORIES: CleanupCategory[] = [
  {
    id: 'virtualMemory',
    label: 'Virtual Memory',
    description: 'Disable hiberfil + enforce pagefile clear',
    icon: 'heat-grid',
    color: '#dc2626',
    severity: 'danger',
    group: 'action-only',
    systemWide: true,
    getDataKey: '',
    clearDataKey: 'invokeVirtualMemoryPurge',
    actionOnly: true,
    confirmMessage: 'Disable hibernation + enforce pagefile clear on shutdown?',
    extractPreview: () => ({ count: -1, items: [] }),
  },
  {
    id: 'unallocatedErase',
    label: 'Free Space Cleanup',
    description: 'Select drives to overwrite — SSD gets cipher + TRIM, HDD gets DoD 3-pass',
    icon: 'delete',
    color: '#b91c1c',
    severity: 'danger',
    group: 'action-only',
    getDataKey: '',
    clearDataKey: 'invokeUnallocatedSpaceErase',
    actionOnly: true,
    extractPreview: () => ({ count: -1, items: [] }),
  },
  {
    id: 'ssdTrim',
    label: 'Force SSD TRIM',
    description: 'Optimize-Volume -ReTrim on all drives',
    icon: 'refresh',
    color: '#991b1b',
    severity: 'danger',
    group: 'action-only',
    getDataKey: '',
    clearDataKey: 'invokeSSDTrim',
    actionOnly: true,
    extractPreview: () => ({ count: -1, items: [] }),
  },
];

// ── View-Only Categories (monitoring, no destructive clear) ──────────────
// These are read-only tools rather than cleanup tools. They show runtime
// system state and belong in a separate read-only section of the panel.

export const VIEW_ONLY_CATEGORIES: CleanupCategory[] = [
  {
    id: 'processIntel',
    label: 'Process Review',
    description: 'Unsigned/elevated running processes',
    icon: 'diagnosis',
    color: '#f97316',
    severity: 'warning',
    group: 'standard',
    getDataKey: 'getProcessIntelligence',
    clearDataKey: '',
    extractPreview: (data) => {
      const procs = Array.isArray(data?.processes) ? data.processes : [];
      return {
        count: procs.length,
        items: procs.map((p: any) => p.name || p.processName || '—'),
      };
    },
  },
];

// ── Schedulable-clearer overlay ──────────────────────────────────────
// Every standard + DFIR card with a clearDataKey gets a per-card auto-erase
// timer in the UI. Action-only (free-space erase, SSD TRIM, virtual memory) and
// view-only (Process Review, SRUM) are explicitly excluded — destructive
// long-running ops shouldn't run on a timer, and view-only has no clearer.
//
// SUPPORTED_AUTOERASE_IDS is the canonical list — MUST match the keys of
// `$script:AutoEraseScripts` in privacy/cleanup.ps1 (and the inline switch
// in commander-pro/src/handlers.rs). Hardcoded here so the clock icon
// appears instantly on first render — no waiting on an async backend
// roundtrip. If you add a new erase script in the PowerShell module, add
// the id here too.
export const SUPPORTED_AUTOERASE_IDS = new Set<string>([
  // Standard
  'clipboard', 'rdpHistory', 'eventLogs', 'recentFiles', 'jumpLists',
  'psHistory', 'dnsCache', 'browserFootprints', 'prefetchFiles',
  'shellBags', 'usbHistory', 'execCache', 'wlanProfiles', 'netDrives',
  'ntfsJournals', 'recycleBin',
  // DFIR
  'ntUserTraces', 'notepadState', 'pcaDatabase', 'crashDumps', 'walFiles',
  'printSpooler', 'webCache', 'notificationDb', 'branchCache',
  'eventTranscript', 'activitiesTimeline', 'rdpBitmapCache', 'servicingLogs',
  'deviceInstallLogs', 'usageTraceLogs',
  // Disk cleanup (scheduled via cleanmgr — runs as current user). No
  // CleanupCategory carries this id, so applyScheduling() never surfaces it
  // here; its only UI is Maintenance's "Reclaim disk space" card, which gates
  // the control on the same `hasPaid && !isInvestigator` rule this panel uses.
  'diskCleanup',
]);

// SYSTEM_CONTEXT_IDS lists clearers that must run as NT AUTHORITY\SYSTEM:
//   - eventLogs: Security log ClearLog right is SYSTEM-only
//   - searchIndex: Windows.db files are TrustedInstaller-protected
//   - amcache: Amcache.hve is locked by the kernel; SYSTEM unlocks it
//   - recallDb: Recall snapshot DBs live in protected app-package dirs
//   - branchCache: BranchCache config/cache is system-level; SYSTEM required
//   - eventTranscript: EventTranscript.db lives under a SYSTEM-owned diagnostic data path
//   - servicingLogs: CBS/DISM logs live under %WINDIR%\Logs, TrustedInstaller-ACL'd
//   - deviceInstallLogs: setupapi logs live under %WINDIR%\INF, SYSTEM-owned
//   - usageTraceLogs: SleepStudy/WDI/WMI ETW traces live under %WINDIR%\System32, SYSTEM-owned
// (See commander-anticleanup-protected-keys for the why.) Most of these
// aren't in SUPPORTED_AUTOERASE_IDS yet because their inline erase scripts
// are non-trivial — but if they are added later, flag them here.
const SYSTEM_CONTEXT_IDS = new Set<string>([
  'eventLogs', 'searchIndex', 'amcache', 'recallDb', 'branchCache',
  'eventTranscript', 'servicingLogs', 'deviceInstallLogs', 'usageTraceLogs',
]);

// Mutate the existing exported arrays in-place so consumers that imported
// `STANDARD_CATEGORIES` / `DEEP_DFIR_CATEGORIES` keep their references and
// just see the new fields populated.
function applyScheduling(cats: CleanupCategory[], minInterval: number) {
  for (const c of cats) {
    if (!c.clearDataKey) continue;
    const schedulerId = c.id === 'clipboardHistory' ? 'clipboard' : c.id;
    if (!SUPPORTED_AUTOERASE_IDS.has(schedulerId)) continue;
    c.schedulerCategoryId = schedulerId;
    c.schedulable = true;
    c.minIntervalMinutes = minInterval;
    c.schedulerRunAsSystem = SYSTEM_CONTEXT_IDS.has(schedulerId);
  }
}

// Standard categories: clipboard / DNS / recent files / jump lists etc.
// are all cheap to erase and there's no reason to forbid a 1-minute
// interval. The backend already validates `IntervalMinutes >= 1`.
// DFIR keeps a 60-min floor — Search Index rebuilds, Amcache rewrites
// and other heavy DFIR ops shouldn't run more than hourly.
applyScheduling(STANDARD_CATEGORIES, 1);
applyScheduling(DEEP_DFIR_CATEGORIES, 60);

export const SCHEDULABLE_CATEGORIES: CleanupCategory[] = [
  ...STANDARD_CATEGORIES,
  ...DEEP_DFIR_CATEGORIES,
];

export const ALL_CATEGORIES = [
  ...STANDARD_CATEGORIES,
  ...DEEP_DFIR_CATEGORIES,
  ...ACTION_CATEGORIES,
  ...VIEW_ONLY_CATEGORIES,
];

// Cleanup source and severity do not describe the cost to the person using
// Windows. Keep that decision explicit, complete, and independent of either.
const USABILITY_TIER_BY_ID: Record<string, CleanupUsabilityTier> = {
  dnsCache: 'low-impact',
  eventLogs: 'low-impact',
  execCache: 'low-impact',
  amcache: 'low-impact',
  crashDumps: 'low-impact',
  srumData: 'low-impact',
  walFiles: 'low-impact',
  branchCache: 'low-impact',
  eventTranscript: 'low-impact',
  servicingLogs: 'low-impact',
  deviceInstallLogs: 'low-impact',
  usageTraceLogs: 'low-impact',
  defenderHistory: 'low-impact',
  remoteAccessLogs: 'low-impact',
  gameLauncherLogs: 'low-impact',
  firewallLog: 'low-impact',

  shellBags: 'history-cache',
  clipboardHistory: 'history-cache',
  psHistory: 'history-cache',
  recentFiles: 'history-cache',
  jumpLists: 'history-cache',
  connectivityHistory: 'history-cache',
  ntUserTraces: 'history-cache',
  recallDb: 'history-cache',
  thumbnailDb: 'history-cache',
  notificationDb: 'history-cache',
  activitiesTimeline: 'history-cache',
  rdpBitmapCache: 'history-cache',
  networkWizardHistory: 'history-cache',
  werHistory: 'history-cache',
  spotlightCache: 'history-cache',
  photosCache: 'history-cache',
  xboxCache: 'history-cache',
  editorHistory: 'history-cache',
  adobeRecent: 'history-cache',
  geolocationCache: 'history-cache',

  usbHistory: 'rebuilds-apps-connectivity',
  rdpHistory: 'rebuilds-apps-connectivity',
  prefetchFiles: 'rebuilds-apps-connectivity',
  ntfsJournals: 'rebuilds-apps-connectivity',
  pcaDatabase: 'rebuilds-apps-connectivity',
  searchIndex: 'rebuilds-apps-connectivity',
  printSpooler: 'rebuilds-apps-connectivity',
  fontCache: 'rebuilds-apps-connectivity',
  legacyIconCache: 'rebuilds-apps-connectivity',
  neighborCache: 'rebuilds-apps-connectivity',
  netbiosCache: 'rebuilds-apps-connectivity',
  proxyCache: 'rebuilds-apps-connectivity',

  browserFootprints: 'data-accounts-recovery',
  recycleBin: 'data-accounts-recovery',
  wlanProfiles: 'data-accounts-recovery',
  netDrives: 'data-accounts-recovery',
  shadowCopies: 'data-accounts-recovery',
  notepadState: 'data-accounts-recovery',
  webCache: 'data-accounts-recovery',
  wslData: 'data-accounts-recovery',
  dockerDesktopData: 'data-accounts-recovery',
  virtualMachineArtifacts: 'data-accounts-recovery',
  developerCaches: 'data-accounts-recovery',
  credentialManager: 'data-accounts-recovery',
  inactiveUserProtectionMetadata: 'data-accounts-recovery',
  stickyNotes: 'data-accounts-recovery',
  oneDriveMetadata: 'data-accounts-recovery',
  gameCaptures: 'data-accounts-recovery',
  communicationCaches: 'data-accounts-recovery',
  gitActivity: 'data-accounts-recovery',
  sshState: 'data-accounts-recovery',
  passwordManagerCaches: 'data-accounts-recovery',
  officeTempFiles: 'data-accounts-recovery',
  vpnPhonebooks: 'data-accounts-recovery',
  cloudPlaceholders: 'data-accounts-recovery',
  bitsQueue: 'data-accounts-recovery',
  cellularHistory: 'data-accounts-recovery',
};

for (const category of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]) {
  const usabilityTier = USABILITY_TIER_BY_ID[category.id];
  if (!usabilityTier) {
    throw new Error(`Missing usability tier for cleanup category: ${category.id}`);
  }
  category.usabilityTier = usabilityTier;
}

export function isLowImpactCategory(category: CleanupCategory): boolean {
  return category.usabilityTier === 'low-impact';
}
