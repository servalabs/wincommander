import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void;
  };
};

test("backup dialog shows the recorded restore location and never labels a legacy receipt as confirmed", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import React from 'react';
    import { renderToStaticMarkup } from 'react-dom/server';
    import { JSDOM } from 'jsdom';
    const h = React.createElement;
    let result;
    mock.module('@/hooks/useVaultCloudBackup', () => ({ default: () => ({ result, busy: false, action: null, error: '', perform: async () => true }) }));
    mock.module('@/components/ui/bp', () => ({
      Button: ({ children, onClick, disabled }) => h('button', { onClick, disabled }, children),
      Dialog: ({ children, className }) => h('div', { className }, children),
      DialogBody: ({ children, className }) => h('div', { className }, children),
      DialogFooter: ({ actions }) => h('footer', null, actions),
    }));
    mock.module('@/components/ui/input', () => ({ Input: props => h('input', props) }));
    const { default: Dialog } = await import('./src/components/shared/VaultCloudBackupDialog');
    const configuration = { whole_vault: false, source_relative_paths: ['Documents'], remote_folder_name: 'Current backup', automatic: true };
    const run = { run_id: 'restored-run', action: 'restore', remote_folder_name: 'Previous backup', restore_relative_path: 'RestoredCheck',
      confirmation: 'restored_sample_matched', checked_files: 1,
      outcome: 'complete', started_at: '2026-11-17T10:00:00Z', finished_at: '2026-11-17T10:01:00Z',
      transferred_items: 2, transferred_bytes: '27', skipped_items: 0, failed_items: 0 };
    const base = { mount_receipt: 'a'.repeat(64), engine_ready: true, connected: true, configured: true,
      automatic: true, watching: true, running: false, can_configure: true, configuration, reason_code: null };
    const render = () => new JSDOM(renderToStaticMarkup(h(Dialog, { isOpen: true, onClose() {}, personal: true, internalDrive: 3, driveLabel: 'X:' })));
    result = { ...base, last_run: run };
    let dom = render();
    const confirmation = dom.window.document.querySelector('.vault-cloud-backup__result');
    assert.equal(confirmation.dataset.tone, 'success');
    assert.match(confirmation.textContent, /restor/i);
    assert.match(confirmation.textContent, /RestoredCheck/);
    assert.doesNotMatch(confirmation.textContent, /Current backup/);
    assert.ok([...dom.window.document.querySelectorAll('details')].every(item => !item.open));
    dom.window.close();
    result = { ...base, last_run: { ...run, action: null, remote_folder_name: null, restore_relative_path: null } };
    dom = render();
    const legacy = dom.window.document.querySelector('.vault-cloud-backup__result');
    assert.notEqual(legacy.dataset.tone, 'success');
    assert.doesNotMatch(legacy.textContent, /RestoredCheck|Current backup/);
    dom.window.close();
    const upload = { ...run, run_id: 'newer-backup', action: 'start', confirmation: 'remote_sample_matched',
      remote_folder_name: 'Current backup', restore_relative_path: null,
      started_at: '2026-11-17T11:00:00Z', finished_at: '2026-11-17T11:01:00Z' };
    result = { ...base, last_run: upload, last_backup: upload, last_verification: null, last_restore: run };
    dom = render();
    const restored = [...dom.window.document.querySelectorAll('.vault-cloud-backup__result')]
      .find(item => /RestoredCheck/.test(item.textContent));
    assert.ok(restored, 'a later automatic backup must not hide the restore confirmation');
    assert.equal(restored.dataset.tone, 'success');
    assert.ok([...dom.window.document.querySelectorAll('button')].some(item => /Last restore:/.test(item.textContent)));
    dom.window.close();
  `;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 30_000);
  try {
    const stderr = await new Response(child.stderr).text();
    const exitCode = await child.exited;
    if (exitCode !== 0) throw new Error(stderr);
    expect(exitCode).toBe(0);
  } finally { clearTimeout(timer); }
}, 35_000);
