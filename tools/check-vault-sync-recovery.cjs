// SPDX-License-Identifier: AGPL-3.0-or-later
// Real recovery dialog with browser-local backend receipts; no native mutations.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw new Error('Fixture requires loopback');
const html = `<!doctype html><html><head><script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {}; window.$RefreshSig$ = () => type => type;
window.__vite_plugin_react_preamble_installed__ = true;
</script></head><body><div id="fixture"></div><script type="module" src="/__sync_recovery_fixture.js"></script></body></html>`;
const backend = `export default function useBackend() { return {
  getEncryptedVolumeStatus: async () => { window.__statusCalls++; return window.__status; },
  enablePersonalVaultSync: async (internalDrive, relativePath, action, token) => {
    window.__calls.push({internalDrive,relativePath,action,token});
    if (action === 'inspect') return window.__inspection;
    if (window.__mode === 'pending') await new Promise(resolve => { window.__resolve = resolve; });
    if (window.__mode === 'failure') throw new Error('vault_mount_state_unknown');
    return {enabled:true,recovery_required:false,pairing_required:true,gui_url:'http://127.0.0.1:51995'};
  }
}; }`;
const fixture = `
import React from '__REACT_MODULE__';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import '/src/index.css';
import '/src/styles/v2-theme.css';
import Dialog from '/src/components/shared/VaultSyncWarningDialog.tsx';
import {notifyPolicyMountSyncWarning,notifyVaultSyncRecovery} from '/src/lib/vaultSyncWarning.ts';
import {setPopupAlertsEnabled} from '/src/lib/notificationStore.ts';
setPopupAlertsEnabled(false);
window.__reset = () => {
  window.__calls=[]; window.__statusCalls=0; window.__mode='success';
  window.__status={success:true,data:{volumes:[{letter:'J:',internalDrive:8,accessible:true}]}};
  window.__inspection={enabled:false,recovery_required:true,gui_url:'http://127.0.0.1:51995',recovery_roots:[
    {relative_path:'Photos',reason:'root_missing',token:'a'.repeat(64)},
    {relative_path:'Calls',reason:'marker_missing',token:'b'.repeat(64)}]};
};
window.__reset();
window.__mount=()=>notifyPolicyMountSyncWarning({state:'mounted',drive_letter:'J:',sync_warning:'recovery_required'});
window.__enroll=()=>notifyVaultSyncRecovery('J:',8,window.__inspection);
ReactDOM.createRoot(document.getElementById('fixture')).render(React.createElement(Dialog));
`;

async function main() {
  const browser = await chromium.launch({headless:true,executablePath:process.env.WINCOMMANDER_BROWSER_EXECUTABLE || undefined});
  const page = await browser.newPage({viewport:{width:1100,height:800}});
  const errors=[]; page.on('pageerror', error=>errors.push(error.message));
  page.setDefaultTimeout(15000);
  try {
    const source=await (await page.request.get(new URL('/src/components/shared/VaultSyncWarningDialog.tsx',origin).href)).text();
    const reactModule=source.match(/from "([^"]*\/react\.js[^"]*)"/)?.[1];
    assert.ok(reactModule);
    await page.route('**/src/hooks/useBackend.ts*',route=>route.fulfill({contentType:'application/javascript',body:backend}));
    await page.route('**/__sync_recovery_fixture.js',route=>route.fulfill({contentType:'application/javascript',body:fixture.replaceAll('__REACT_MODULE__',reactModule)}));
    await page.route('**/__sync_recovery__',route=>route.fulfill({contentType:'text/html',body:html}));
    await page.goto(new URL('/__sync_recovery__',origin).href);
    await page.waitForFunction(()=>typeof window.__mount==='function');
    // A mount inspects actual identity but never recreates before a decision.
    await page.evaluate(()=>window.__mount());
    const dialog=page.getByRole('dialog');
    await dialog.getByText('J:\\Photos',{exact:true}).waitFor();
    if (process.env.WINCOMMANDER_RECOVERY_SCREENSHOT) {
      await page.screenshot({path:process.env.WINCOMMANDER_RECOVERY_SCREENSHOT,fullPage:true});
    }
    assert.equal(await page.evaluate(()=>window.__calls.length),1);
    assert.equal(await page.evaluate(()=>window.__calls[0].action),'inspect');
    await dialog.getByRole('button',{name:'Keep paused',exact:true}).first().click();
    assert.equal(await page.evaluate(()=>window.__calls.length),1);
    await dialog.getByRole('button',{name:'Recreate sync setup',exact:true}).click();
    await dialog.getByText(/Calls: sync setup recreated/).waitFor();
    assert.deepEqual(await page.evaluate(()=>window.__calls[1]),{internalDrive:8,relativePath:'Calls',action:'recreate',token:'b'.repeat(64)});
    assert.match(await dialog.innerText(),/deleted files were not restored/);
    await dialog.getByRole('button',{name:'Done',exact:true}).click();
    await dialog.waitFor({state:'hidden'});
    // Direct enrollment supplies authoritative tokens without guessing a path.
    await page.evaluate(()=>{window.__reset();window.__enroll();});
    await dialog.getByText('J:\\Photos',{exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>window.__statusCalls),0);
    await page.evaluate(()=>{window.__mode='failure';});
    await dialog.getByRole('button',{name:'Recreate sync setup',exact:true}).first().click();
    await dialog.getByRole('alert').waitFor();
    assert.equal(await dialog.getByRole('button',{name:'Recreate sync setup',exact:true}).count(),0);
    assert.match(await dialog.getByRole('alert').innerText(),/still mounted/);
    await dialog.getByRole('button',{name:'Check again',exact:true}).click();
    await dialog.getByText('J:\\Photos',{exact:true}).waitFor();
    // Pending operations cannot be submitted twice or dismissed as completed.
    await page.evaluate(()=>{window.__mode='pending';});
    await dialog.getByRole('button',{name:'Recreate sync setup',exact:true}).first().click();
    await page.waitForFunction(()=>typeof window.__resolve==='function');
    assert.equal(await dialog.getByRole('button',{name:'Done',exact:true}).isDisabled(),true);
    await page.keyboard.press('Escape');
    assert.equal(await dialog.isVisible(),true);
    await page.evaluate(()=>{window.__mode='success';window.__resolve();});
    await dialog.getByText(/Photos: sync setup recreated/).waitFor();
    await dialog.getByRole('button',{name:'Done',exact:true}).click();
    await dialog.waitFor({state:'hidden'});
    // Unknown ownership/availability must never expose a mutation target.
    await page.evaluate(()=>{window.__reset();window.__status={success:true,data:{volumes:[]}};window.__mount();});
    await dialog.getByRole('alert').waitFor();
    assert.equal(await dialog.getByRole('button',{name:'Recreate sync setup',exact:true}).count(),0);
    assert.equal(await page.evaluate(()=>window.__calls.length),0);
    await dialog.getByRole('button',{name:'Done',exact:true}).click();
    await dialog.waitFor({state:'hidden'});
    // A full set remains scrollable and the exit action stays in the viewport.
    await page.evaluate(()=>{
      window.__reset();
      window.__inspection.recovery_roots=Array.from({length:32},(_,i)=>({relative_path:'Folder'+i,reason:'configuration_missing',token:i.toString(16).padStart(64,'0')}));
      window.__enroll();
    });
    await dialog.getByText('J:\\Folder0',{exact:true}).waitFor();
    assert.equal(await dialog.getByRole('button',{name:'Recreate sync setup',exact:true}).count(),32);
    const footer=await dialog.getByRole('button',{name:'Done',exact:true}).boundingBox();
    assert.ok(footer && footer.y>=0 && footer.y+footer.height<=800);
    await dialog.getByRole('button',{name:'Keep paused',exact:true}).last().click();
    assert.equal(await page.evaluate(()=>window.__calls.length),0);
    assert.deepEqual(errors,[]);
    console.log('Vault sync recovery browser fixture passed: consent, multi-root targeting, failure, busy guard and unknown identity.');
  } finally {await browser.close();}
}
main().catch(error=>{console.error(error);process.exitCode=1;});
