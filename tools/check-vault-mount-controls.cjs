// Real Secure Storage components; every backend operation is simulated.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw Error('Loopback fixture required');
const html = `<!doctype html><script type="module">import RefreshRuntime from '/@react-refresh';RefreshRuntime.injectIntoGlobalHook(window);window.$RefreshReg$=()=>{};window.$RefreshSig$=()=>type=>type;window.__vite_plugin_react_preamble_installed__=true;</script><div id="fixture"></div><script type="module" src="/__vault_controls.js"></script>`;
const fixture = `
import React from 'REACT_URL';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import {QueryClient,QueryClientProvider} from 'QUERY_URL';
import '/src/index.css'; import '/src/styles/v2-theme.css';
import VaultPanel from '/src/panels/vault/index.tsx';
import useVaultDriveLetters from '/src/hooks/useVaultDriveLetters.ts';
import {Dialog,DialogContent,DialogHeader,DialogTitle} from '/src/components/ui/dialog.tsx';
import VaultOperationNotice from '/src/components/shared/VaultOperationNotice.tsx';
import '/src/components/RightSidebar.css';
window.__pendingLetters=[];window.__letterRequests=0;window.__pendingPartitions=[];window.__mountReason='vault_administrator_required';window.__toasts=[];
window.__resolveLetters=result=>{for(const resolve of window.__pendingLetters.splice(0))resolve(result)};
window.__backend={
  getAvailableDriveLetters:()=>{window.__letterRequests++;return new Promise(resolve=>window.__pendingLetters.push(resolve))},
  getEncryptionPartitions:()=>window.__pausePartitions ? new Promise(resolve=>window.__pendingPartitions.push(resolve)) : Promise.resolve({success:true,data:{partitions:[]}}),
  mountVolume:()=>window.__pauseMount ? new Promise(resolve=>window.__finishMount=resolve) : Promise.resolve({success:false,error:window.__mountReason}),
  getEncryptedVolumeStatus:async()=>({success:true,data:window.__state.encryptionStatus}),
  dismountVolume:async()=>({success:false,error:window.__mountReason}),
};
window.__state={encryptionStatus:{volumes:[]},vaultStatusError:null,vaultManualRefreshing:false,loading:{vault:false},refreshVault:async()=>{window.__state.loading.vault=true;window.__state.vaultManualRefreshing=true;window.__rerender();await new Promise(resolve=>window.__finishRefresh=resolve);window.__state.loading.vault=false;window.__state.vaultManualRefreshing=false;window.__rerender();return window.__state.encryptionStatus}};
function Fixture(){useVaultDriveLetters(true);const[,update]=React.useState(0);const[show,setShow]=React.useState(false);window.__rerender=()=>update(n=>n+1);return React.createElement(React.Fragment,null,React.createElement(VaultPanel),React.createElement('button',{onClick:()=>setShow(true)},'Show dismount feedback'),React.createElement(Dialog,{open:show,onOpenChange:setShow},React.createElement(DialogContent,{className:'vault-dismount-dialog'},React.createElement(DialogHeader,null,React.createElement(DialogTitle,null,'Dismount needs attention')),React.createElement(VaultOperationNotice,{message:'2 encrypted volumes dismounted; 1 left mounted. Your Windows account does not have the required Fleet Vault permission. Ask the Vault owner to review your access.'}))))}
const queryClient=new QueryClient({defaultOptions:{queries:{retry:false}}});
ReactDOM.createRoot(document.getElementById('fixture')).render(React.createElement(QueryClientProvider,{client:queryClient},React.createElement(Fixture)));`;

(async()=>{
  const browser=await chromium.launch({headless:true});
  const page=await browser.newPage({viewport:{width:1280,height:1000}});
  page.setDefaultTimeout(15000);
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  try {
    const source=await(await page.request.get(new URL('/src/panels/vault/index.tsx',origin).href)).text();
    const react=source.match(/from "([^"]*\/react\.js[^"]*)"/)?.[1];assert.ok(react);
    const querySource=await(await page.request.get(new URL('/src/hooks/useVaultDriveLetters.ts',origin).href)).text();
    const query=querySource.match(/from "([^"]*@tanstack_react-query\.js[^"]*)"/)?.[1];assert.ok(query);
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
    await module('**/__vault_controls.js',fixture.replace('REACT_URL',react).replace('QUERY_URL',query));
    await page.route('**/__vault_controls__',route=>route.fulfill({contentType:'text/html',body:html}));
    await page.goto(new URL('/__vault_controls__',origin).href);
    await page.waitForFunction(()=>window.__letterRequests===1&&window.__pendingLetters.length===1);
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:['J','Z']}}));
    await page.waitForFunction(()=>window.__pendingLetters.length===0);
    const refresh=page.getByRole('button',{name:'Refresh encryption volume status',exact:true});
    await refresh.click();assert.equal(await refresh.isDisabled(),true,'Manual refresh remains busy until its IPC settles');
    await page.evaluate(()=>window.__finishRefresh());await page.waitForFunction(()=>!window.__state.loading.vault);
    await page.evaluate(()=>{window.__pausePartitions=true});
    await page.getByRole('button',{name:'Mount Volume',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingPartitions.length===1);
    assert.equal(await page.evaluate(()=>window.__letterRequests),1,'Opening subscribes to the startup cache without another drive-letter request');
    await page.getByRole('dialog').locator('#volume-path').fill('D:\\Fixture\\example.ec');
    await page.getByRole('dialog').locator('#password').fill('fixture-only');
    assert.equal(await page.getByRole('button',{name:'MOUNT VOLUME',exact:true}).isEnabled(),true,'File containers must not wait for partition discovery');
    await page.getByRole('button',{name:'CANCEL',exact:true}).click();
    await page.getByRole('button',{name:'Mount Volume',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingPartitions.length===2);
    await page.evaluate(()=>{
      window.__pendingPartitions[1]({success:true,data:{partitions:[]}});
    });
    assert.equal(await page.evaluate(()=>window.__letterRequests),1,'Closing and reopening reuses the same startup drive-letter result');
    await page.getByRole('dialog').getByRole('tab',{name:'Partition / Drive',exact:true}).click();
    await page.getByRole('dialog').getByText('No mountable partitions found.',{exact:true}).waitFor();
    await page.evaluate(()=>{window.__toasts=[];window.__pendingPartitions[0]({success:false,error:'OLD_PARTITION_FAILURE'});window.__pausePartitions=false});
    await page.waitForTimeout(100);
    assert.equal(await page.evaluate(()=>window.__toasts.length),0,'A closed dialog cannot report an old partition failure into the new dialog');
    await page.getByRole('button',{name:'CANCEL',exact:true}).click();
    await page.getByRole('button',{name:'Mount Volume',exact:true}).click();
    const dialog=page.getByRole('dialog');
    assert.equal(await page.evaluate(()=>window.__letterRequests),1,'Opening the dialog never performs an on-open letter fetch');
    const letterGroup=dialog.getByRole('radiogroup',{name:'Drive letter',exact:true});
    const letter=letterGroup.getByRole('radio',{name:'J:',exact:true});
    await letter.waitFor();
    await letter.click();
    assert.equal(await letter.getAttribute('aria-checked'),'true');
    assert.equal(await letterGroup.getByRole('radio',{name:'C:',exact:true}).count(),0);
    assert.equal(await dialog.getByRole('combobox',{name:'Drive letter',exact:true}).count(),0,'Secure Storage uses the grid while other picker callers retain the dropdown default');
    assert.equal(await letter.evaluate(element=>getComputedStyle(element).borderTopWidth),'1px');
    await dialog.locator('#volume-path').fill('D:\\Fixture\\example.ec');
    await dialog.locator('#password').fill('fixture-only');
    await letterGroup.getByRole('radio',{name:'Z:',exact:true}).press('Enter');
    assert.equal(await letterGroup.getByRole('radio',{name:'Z:',exact:true}).getAttribute('aria-checked'),'true','Enter selects the focused letter instead of submitting the previous letter');
    assert.equal(await page.evaluate(()=>window.__letterRequests),1,'Selecting a radio with Enter must not start mount preflight');
    await letter.press('Enter');
    await page.evaluate(()=>{window.__pauseMount=true});
    await dialog.getByRole('button',{name:'MOUNT VOLUME',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingLetters.length>0);
    assert.equal(await page.evaluate(()=>window.__letterRequests),2,'Mount submit performs a fresh preflight drive-letter request');
    assert.equal(await dialog.getByRole('checkbox',{name:'Mount read-only',exact:true}).isDisabled(),true,'Mount access mode must be frozen during preflight');
    assert.equal(await letter.isDisabled(),true,'The submitted drive must stay frozen');
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:['J','Z']}}));
    await dialog.getByRole('status').filter({hasText:'Waiting for the Vault service'}).waitFor();
    assert.equal(await dialog.getByRole('button',{name:'CANCEL',exact:true}).isDisabled(),true);
    assert.equal(await dialog.locator('#volume-path').isDisabled(),true);
    assert.equal(await dialog.getByRole('checkbox',{name:'Mount read-only',exact:true}).isDisabled(),true);
    assert.equal(await dialog.locator('#password').inputValue(),'','Clear the visible password immediately after dispatch');
    await page.evaluate(()=>{window.__pauseMount=false;window.__finishMount({success:false,error:window.__mountReason})});
    const failure=dialog.getByRole('alert').filter({hasText:'administrator approval'});
    await failure.waitFor();
    assert.equal(await failure.evaluate(element=>getComputedStyle(element).backgroundColor),'rgb(255, 241, 242)');
    assert.equal((await failure.innerText()).includes('password'),false);
    await page.evaluate(()=>{
      window.__toasts=[];
      window.__savedRefresh=window.__state.refreshVault;
      window.__state.refreshVault=async()=>null;
      window.__backend.mountVolume=async()=>({success:true,data:{status:'mounted',drive:'J:',scope:'machine',internalDrive:4}});
      window.__backend.verifyVaultDrive=async()=>({drive:'J:',accessible:true});
      window.__rerender();
    });
    await dialog.locator('#password').fill('fixture-only');
    await dialog.getByRole('button',{name:'MOUNT VOLUME',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingLetters.length>0);
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:['J','Z']}}));
    const observationFailure=dialog.getByRole('alert').filter({hasText:'mount and Windows drive access were confirmed'});
    await observationFailure.waitFor();
    assert.equal((await observationFailure.innerText()).includes('does not mean mounting failed'),true);
    assert.equal(await page.evaluate(()=>window.__toasts.some(toast=>toast.kind==='success')),false,'Unavailable inventory cannot produce an all-clear success notification');
    await page.evaluate(()=>{window.__state.refreshVault=window.__savedRefresh;window.__backend.mountVolume=async()=>({success:false,error:window.__mountReason});window.__rerender()});
    await dialog.getByRole('button',{name:'Refresh free letters',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingLetters.length===1);
    await page.evaluate(()=>window.__resolveLetters({success:false,error:'unavailable'}));
    await dialog.getByRole('alert').filter({hasText:'could not be checked'}).waitFor();
    assert.equal(await dialog.getByText(/No free drive letters/).count(),0,'Failed discovery is not exhausted');
    await dialog.getByRole('button',{name:'Refresh free letters',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingLetters.length===1);
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:[]}}));
    await dialog.getByRole('alert').filter({hasText:'No free drive letters'}).waitFor();
    await dialog.getByRole('button',{name:'Refresh free letters',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingLetters.length===1);
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:['J']}}));
    await letter.waitFor();
    assert.equal(await dialog.getByText(/No free drive letters/).count(),0);
    await dialog.getByRole('button',{name:'CANCEL',exact:true}).click();
    await page.evaluate(()=>{window.__state.encryptionStatus={volumes:[{letter:'J:',type:'Normal',path:'D:\\Fixture\\example.ec',accessible:true,internalDrive:2}]};window.__rerender()});
    await page.getByRole('button',{name:'Force dismount J:',exact:true}).click();
    const rowError=page.getByRole('alert').filter({hasText:'administrator approval'});
    await rowError.waitFor();
    assert.equal(await rowError.evaluate(element=>element.parentElement.colSpan),4,'Error occupies the full table row, not the narrow action column');
    await page.evaluate(()=>{window.__state.vaultStatusError='Mounted-volume status could not be refreshed. Last confirmed drives may have changed.';window.__rerender()});
    await page.getByText('Last confirmed drives — actions are paused until status can be checked.',{exact:true}).waitFor();
    assert.equal(await page.getByRole('button',{name:'Force dismount J:',exact:true}).isDisabled(),true,'Stale displayed rows cannot authorize actions');
    await page.evaluate(()=>{window.__state.vaultStatusError=null;window.__rerender()});
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
    console.log('PASS: startup drive-letter preload/cache with no open/reopen fetch; manual and mount-preflight refresh; Secure Storage radio grid; loading/unavailable/empty/selected states; red administrative error separate from help; full-row error and bounded dismount modal at1440/1024/880 widths; busy refresh; unavailable status not empty.');
  } catch(error) {console.error({errors,body:await page.locator('body').innerText()});throw error}
  finally{await browser.close()}
})().catch(error=>{console.error(error);process.exitCode=1});
