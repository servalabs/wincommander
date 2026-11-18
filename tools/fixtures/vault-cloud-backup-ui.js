// Browser-only fixture. No account, credentials, actual Vault or native mutation.
import React from 'react';
import { createRoot } from 'react-dom/client';
import '../../src/index.css';
import '../../src/styles/v2-theme.css';
import VaultCloudBackupDialog from '../../src/components/shared/VaultCloudBackupDialog';

let root;
export function renderCloudBackupFixture(state = 'new') {
  root?.unmount();
  const configuration = { whole_vault: false, source_relative_paths: ['Documents'], remote_folder_name: 'Example backup', automatic: true };
  const configured = state !== 'new';
  window.__cloudRequests = [];
  window.__cloudState = {
    mount_receipt: 'a'.repeat(64), engine_ready: configured, connected: configured,
    configured, automatic: configured, watching: configured, running: false, can_configure: true,
    configuration: configured ? configuration : null, last_run: state === 'partial' ? {
      run_id: 'fixture-run', outcome: 'partial', started_at: '2026-11-16T12:00:00Z', finished_at: '2026-11-16T12:00:10Z',
      transferred_items: 2, transferred_bytes: '1024', skipped_items: 1, failed_items: 1,
      action: 'start', remote_folder_name: 'Example backup', restore_relative_path: null, confirmation: null, checked_files: 0,
    } : null, reason_code: null,
  };
  const call = async request => {
    window.__cloudRequests.push(structuredClone(request));
    if (request.action === 'prepare') window.__cloudState.engine_ready = true;
    if (request.action === 'connect') window.__cloudState.connected = true;
    if (request.action === 'configure') {
      Object.assign(window.__cloudState, { configured: true, configuration: request.configuration,
        automatic: request.configuration.automatic, watching: request.configuration.automatic });
    }
    if (request.action === 'stop') {
      Object.assign(window.__cloudState, { watching: false, running: false });
      return { ...structuredClone(window.__cloudState), engine_ready: false, connected: false, configured: false,
        automatic: false, configuration: null, last_run: null };
    }
    if (request.action === 'disconnect') Object.assign(window.__cloudState, { connected: false, watching: false, running: false });
    if (request.action === 'start') window.__cloudState.running = true;
    return structuredClone(window.__cloudState);
  };
  window.__cloudBackend = { setupVaultCloudBackup: call, manageVaultCloudBackup: call };
  root = createRoot(document.getElementById('fixture'));
  root.render(React.createElement(VaultCloudBackupDialog, {
    isOpen: true, onClose: () => root.unmount(), internalDrive: 6, driveLabel: 'Example Vault', personal: true,
  }));
}
