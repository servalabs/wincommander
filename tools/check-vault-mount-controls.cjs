// Real Secure Storage components; every backend operation is simulated.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw Error('Loopback fixture required');
const html = `<!doctype html><script type="module">import RefreshRuntime from '/@react-refresh';RefreshRuntime.injectIntoGlobalHook(window);window.$RefreshReg$=()=>{};window.$RefreshSig$=()=>type=>type;window.__vite_plugin_react_preamble_installed__=true;</script><div id="fixture"></div><script type="module" src="/__vault_controls.js"></script>`;
const fixture = `
import React from 'REACT_URL';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import '/src/index.css'; import '/src/styles/v2-theme.css';
import VaultPanel from '/src/panels/vault/index.tsx';
import {Dialog,DialogContent,DialogHeader,DialogTitle} from '/src/components/ui/dialog.tsx';
import VaultOperationNotice from '/src/components/shared/VaultOperationNotice.tsx';
import '/src/components/RightSidebar.css';
window.__pendingLetters=[];window.__mountReason='vault_administrator_required';window.__toasts=[];
window.__resolveLetters=result=>{for(const resolve of window.__pendingLetters.splice(0))resolve(result)};
window.__backend={
  getAvailableDriveLetters:()=>new Promise(resolve=>window.__pendingLetters.push(resolve)),
  getEncryptionPartitions:async()=>({success:true,data:{partitions:[]}}),
  mountVolume:async()=>({success:false,error:window.__mountReason}),
  getEncryptedVolumeStatus:async()=>({success:true,data:window.__state.encryptionStatus}),
  dismountVolume:async()=>({success:false,error:window.__mountReason}),
};
window.__state={encryptionStatus:{volumes:[]},loading:{vault:false},refreshVault:async()=>{window.__state.loading.vault=true;window.__rerender();await new Promise(resolve=>window.__finishRefresh=resolve);window.__state.loading.vault=false;window.__rerender();return window.__state.encryptionStatus}};
function Fixture(){const[,update]=React.useState(0);const[show,setShow]=React.useState(false);window.__rerender=()=>update(n=>n+1);return React.createElement(React.Fragment,null,React.createElement(VaultPanel),React.createElement('button',{onClick:()=>setShow(true)},'Show dismount feedback'),React.createElement(Dialog,{open:show,onOpenChange:setShow},React.createElement(DialogContent,{className:'vault-dismount-dialog'},React.createElement(DialogHeader,null,React.createElement(DialogTitle,null,'Dismount needs attention')),React.createElement(VaultOperationNotice,{message:'2 encrypted volumes dismounted; 1 left mounted. Your Windows account does not have the required Fleet Vault permission. Ask the Vault owner to review your access.'}))))}
ReactDOM.createRoot(document.getElementById('fixture')).render(React.createElement(Fixture));`;

(async()=>{
  const browser=await chromium.launch({headless:true});
  const page=await browser.newPage({viewport:{width:1280,height:1000}});
  page.setDefaultTimeout(15000);
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  try {
    const source=await(await page.request.get(new URL('/src/panels/vault/index.tsx',origin).href)).text();
    const react=source.match(/from "([^"]*\/react\.js[^"]*)"/)?.[1];assert.ok(react);
    const module=async(pattern,body)=>page.route(pattern,route=>route.fulfill({contentType:'application/javascript',body}));
    await module('**/src/hooks/useBackend.ts*','export default()=>window.__backend;');
    await module('**/src/context/AppContext.tsx*','export const useAppState=()=>window.__state;export const useOptionalAppState=()=>null;');
    await module('**/src/context/ThemeContext.tsx*',"export const useTheme=()=>({theme:'light'});");
    await module('**/src/hooks/useEntitlements.ts*','export default()=>({canUse:()=>true});');
    await module('**/src/components/shared/AppConfirmDialog.tsx*','export const useAppConfirm=()=>async()=>true;');
    await module('**/src/utils/toast.ts*',"export const showError=message=>window.__toasts.push({kind:'error',message});export const showSuccess=message=>window.__toasts.push({kind:'success',message});");
    await module('**/src/components/shared/TierGate.tsx*','export default({children})=>children;');
    await module('**/src/components/shared/PanelHeader.tsx*','export default()=>null;');
    for(const name of ['CreateVolumeWizard','SystemEncryptionSection','RamDisksSection','StegoBackupSection','VolumePropertiesDialog'])await module('**/src/panels/vault/'+name+'.tsx*','export default()=>null;');
    await module('**/__vault_controls.js',fixture.replace('REACT_URL',react));
    await page.route('**/__vault_controls__',route=>route.fulfill({contentType:'text/html',body:html}));
    await page.goto(new URL('/__vault_controls__',origin).href);
    const refresh=page.getByRole('button',{name:'Refresh encryption volume status',exact:true});
    await refresh.click();assert.equal(await refresh.isDisabled(),true,'Manual refresh remains busy until its IPC settles');
    await page.evaluate(()=>window.__finishRefresh());await page.waitForFunction(()=>!window.__state.loading.vault);
    await page.getByRole('button',{name:'Mount Volume',exact:true}).click();
    const dialog=page.getByRole('dialog');
    await dialog.getByText('Checking available drive letters…',{exact:true}).waitFor();
    assert.equal(await dialog.getByText(/No free drive letters/).count(),0,'Unloaded is not exhausted');
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:['J','Z']}}));
    const letter=dialog.getByRole('combobox',{name:'Drive letter',exact:true});
    await letter.locator('option[value="J"]').waitFor({state:'attached'});
    await letter.selectOption('J');
    assert.equal(await letter.inputValue(),'J');
    assert.equal(await letter.locator('option[value="C"]').count(),0);
    assert.equal(await letter.evaluate(element=>getComputedStyle(element).borderTopWidth),'1px');
    await dialog.locator('#volume-path').fill('D:\\Fixture\\example.ec');
    await dialog.locator('#password').fill('fixture-only');
    await dialog.getByRole('button',{name:'MOUNT VOLUME',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingLetters.length>0);
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:['J','Z']}}));
    const failure=dialog.getByRole('alert').filter({hasText:'administrator approval'});
    await failure.waitFor();
    assert.equal(await failure.evaluate(element=>getComputedStyle(element).backgroundColor),'rgb(255, 241, 242)');
    assert.equal((await failure.innerText()).includes('password'),false);
    await dialog.getByRole('button',{name:'Refresh free letters',exact:true}).click();
    await page.evaluate(()=>window.__resolveLetters({success:false,error:'unavailable'}));
    await dialog.getByRole('alert').filter({hasText:'could not be checked'}).waitFor();
    assert.equal(await dialog.getByText(/No free drive letters/).count(),0,'Failed discovery is not exhausted');
    await dialog.getByRole('button',{name:'Refresh free letters',exact:true}).click();
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:[]}}));
    await dialog.getByRole('alert').filter({hasText:'No free drive letters'}).waitFor();
    await dialog.getByRole('button',{name:'Refresh free letters',exact:true}).click();
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:['J']}}));
    await letter.locator('option[value="J"]').waitFor({state:'attached'});
    assert.equal(await dialog.getByText(/No free drive letters/).count(),0);
    await dialog.getByRole('button',{name:'CANCEL',exact:true}).click();
    await page.evaluate(()=>{window.__state.encryptionStatus={volumes:[{letter:'J:',type:'Normal',path:'D:\\Fixture\\example.ec',accessible:true,internalDrive:2}]};window.__rerender()});
    await page.getByRole('button',{name:'Force dismount J:',exact:true}).click();
    const rowError=page.getByRole('alert').filter({hasText:'administrator approval'});
    await rowError.waitFor();
    assert.equal(await rowError.evaluate(element=>element.parentElement.colSpan),4,'Error occupies the full table row, not the narrow action column');
    for(const viewport of [{width:1440,height:1000},{width:1024,height:600},{width:880,height:520}]) {
      await page.setViewportSize(viewport);
      const box=await rowError.boundingBox();assert.ok(box.width>300,'Row error remains readable at scaled viewport');assert.ok(box.height<200,'Error does not become a vertical strip');
      await page.getByRole('button',{name:'Show dismount feedback',exact:true}).click();
      const modal=page.getByRole('dialog');await modal.waitFor();const bounds=await modal.boundingBox();
      assert.ok(bounds.width<=561&&bounds.width<=viewport.width-30,'Dismount modal remains bounded');assert.ok(bounds.x>=0&&bounds.y>=0&&bounds.y+bounds.height<=viewport.height,'Dialog fits viewport');
      await modal.getByRole('button',{name:'Close',exact:true}).click();
    }
    await page.evaluate(()=>{window.__state.encryptionStatus=null;window.__state.loading.vault=false;window.__rerender()});
    await page.getByRole('alert').filter({hasText:'Mounted-volume status could not be checked'}).waitFor();
    assert.equal(await page.getByText('No volumes mounted',{exact:true}).count(),0);
    assert.deepEqual(errors,[]);
    console.log('PASS: Secure Storage preload/loading/unavailable/empty/selected drive states; service-only dropdown; red administrative error separate from help; full-row error and bounded dismount modal at1440/1024/880 widths; busy refresh; unavailable status not empty.');
  } catch(error) {console.error({errors,body:await page.locator('body').innerText()});throw error}
  finally{await browser.close()}
})().catch(error=>{console.error(error);process.exitCode=1});
