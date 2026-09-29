// Real RAM-disk dialogs and shared letter cache; every backend operation is simulated.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw Error('Loopback fixture required');
const html = `<!doctype html><script type="module">import RefreshRuntime from '/@react-refresh';RefreshRuntime.injectIntoGlobalHook(window);window.$RefreshReg$=()=>{};window.$RefreshSig$=()=>type=>type;window.__vite_plugin_react_preamble_installed__=true;</script><div id="fixture"></div><script type="module" src="/__ramdisk_letters.js"></script>`;
const fixture = `
import React from 'REACT_URL';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import {QueryClient,QueryClientProvider} from 'QUERY_URL';
import '/src/index.css';import '/src/styles/v2-theme.css';import '/src/panels/vault/index.css';
import RamDisksSection from '/src/panels/vault/RamDisksSection.tsx';
import DriveLetterPicker from '/src/panels/vault/DriveLetterPicker.tsx';
import useVaultDriveLetters from '/src/hooks/useVaultDriveLetters.ts';
window.__letterRequests=0;window.__pendingLetters=[];window.__creates=[];window.__saves=[];window.__toasts=[];
window.__resolveLetters=result=>{for(const resolve of window.__pendingLetters.splice(0))resolve(result)};
window.__backend={
  getAvailableDriveLetters:()=>{window.__letterRequests++;return new Promise(resolve=>window.__pendingLetters.push(resolve))},
  getRamDiskStatus:async()=>({success:true,data:{installed:true,disks:[]}}),
  getSystemRamInfo:async()=>({success:true,data:{totalMB:16384,freeMB:8192}}),
  createRamDisk:async request=>{window.__creates.push(request);return {success:true}},
};
window.__state={appSettings:{app:{vault:{ramdiskAutostart:{enabled:true,sizeMB:512,driveLetter:'J'}}}},patchAppSettings:async patch=>window.__saves.push(patch)};
function Fixture(){useVaultDriveLetters(true);const[letter,setLetter]=React.useState('J');return React.createElement(React.Fragment,null,React.createElement(RamDisksSection),React.createElement('section',{'aria-label':'Default dropdown'},React.createElement(DriveLetterPicker,{id:'default-letter',value:letter,letters:['J','Z'],onChange:setLetter})))}
const queryClient=new QueryClient({defaultOptions:{queries:{retry:false}}});
ReactDOM.createRoot(document.getElementById('fixture')).render(React.createElement(QueryClientProvider,{client:queryClient},React.createElement(Fixture)));`;

(async()=>{
  const browser=await chromium.launch({headless:true});
  const page=await browser.newPage({viewport:{width:1280,height:1000}});
  page.setDefaultTimeout(15000);
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  try {
    const source=await(await page.request.get(new URL('/src/panels/vault/RamDisksSection.tsx',origin).href)).text();
    const react=source.match(/from "([^"]*\/react\.js[^"]*)"/)?.[1];assert.ok(react);
    const querySource=await(await page.request.get(new URL('/src/hooks/useVaultDriveLetters.ts',origin).href)).text();
    const query=querySource.match(/from "([^"]*@tanstack_react-query\.js[^"]*)"/)?.[1];assert.ok(query);
    const module=async(pattern,body)=>page.route(pattern,route=>route.fulfill({contentType:'application/javascript',body}));
    await module('**/src/hooks/useBackend.ts*','export default()=>window.__backend;');
    await module('**/src/context/AppContext.tsx*','export const useAppState=()=>window.__state;export const useOptionalAppState=()=>null;');
    await module('**/src/context/ThemeContext.tsx*',"export const useTheme=()=>({theme:'light'});");
    await module('**/src/hooks/useMotionPreference.ts*',"export default()=> 'reduced';");
    await module('**/src/components/shared/AppConfirmDialog.tsx*','export const useAppConfirm=()=>async()=>true;');
    await module('**/src/components/shared/UniversalToggle.tsx*','export default()=>null;');
    await module('**/src/utils/toast.ts*',"export const showError=message=>window.__toasts.push(message);export const showSuccess=message=>window.__toasts.push(message);export const showInfo=message=>window.__toasts.push(message);");
    await module('**/__ramdisk_letters.js',fixture.replace('REACT_URL',react).replace('QUERY_URL',query));
    await page.route('**/__ramdisk_letters__',route=>route.fulfill({contentType:'text/html',body:html}));
    await page.goto(new URL('/__ramdisk_letters__',origin).href);
    await page.waitForFunction(()=>window.__pendingLetters.length===1);
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:['J','Z']}}));
    await page.getByRole('button',{name:'Create RAM Disk',exact:true}).click();
    const dialog=page.getByRole('dialog');
    const grid=dialog.getByRole('radiogroup',{name:'Drive letter',exact:true});
    await grid.waitFor();
    assert.equal(await dialog.getByRole('combobox',{name:'Drive letter',exact:true}).count(),0);
    assert.equal(await grid.getByRole('radio',{name:'C:',exact:true}).count(),0,'Occupied letters are absent');
    assert.equal(await page.evaluate(()=>window.__letterRequests),1,'Opening reuses the startup cache');
    assert.equal(await grid.getByRole('radio',{name:'J:',exact:true}).getAttribute('aria-checked'),'true','Opening selects a free letter when preferred R is occupied');
    await grid.getByRole('radio',{name:'Z:',exact:true}).press('Enter');
    assert.equal(await grid.getByRole('radio',{name:'Z:',exact:true}).getAttribute('aria-checked'),'true');
    assert.equal(await page.evaluate(()=>window.__creates.length),0,'Enter only selects the focused letter');
    await dialog.getByRole('button',{name:'CREATE',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingLetters.length===1);
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:['J','Z']}}));
    await page.waitForFunction(()=>window.__creates.length===1);
    assert.equal(await page.evaluate(()=>window.__creates[0].DriveLetter),'Z','Creation uses the chosen letter after a fresh check');
    await page.getByRole('button',{name:'Create RAM Disk',exact:true}).click();
    await grid.waitFor();
    assert.equal(await grid.getByRole('radio',{name:'J:',exact:true}).getAttribute('aria-checked'),'true','Reopening retains a valid cache-derived default');
    assert.equal(await page.evaluate(()=>window.__letterRequests),2,'Reopening does not read letters again');
    await dialog.getByRole('button',{name:'CANCEL',exact:true}).click();
    await page.getByRole('button',{name:'Edit auto-create settings',exact:true}).click();
    await grid.waitFor();
    assert.equal(await dialog.getByRole('combobox',{name:'Drive letter',exact:true}).count(),0);
    assert.equal(await grid.getByRole('radio',{name:'J:',exact:true}).getAttribute('aria-checked'),'true','Saved startup letter is selected');
    await grid.getByRole('radio',{name:'Z:',exact:true}).press('Enter');
    assert.equal(await page.evaluate(()=>window.__saves.length),0,'Selecting a startup letter does not save automatically');
    for(const viewport of [{width:1280,height:1000},{width:880,height:520},{width:600,height:440}]) {
      await page.setViewportSize(viewport);
      const box=await grid.boundingBox();assert.ok(box.x>=0&&box.x+box.width<=viewport.width,'Grid remains inside viewport');
      assert.equal(await grid.evaluate(element=>element.scrollWidth<=element.clientWidth),true,'Grid never clips horizontally');
    }
    await dialog.getByRole('button',{name:'Save spec',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingLetters.length===1);
    await page.evaluate(()=>window.__resolveLetters({success:true,data:{letters:['J','Z']}}));
    await page.waitForFunction(()=>window.__creates.length===2);
    assert.equal(await page.evaluate(()=>window.__saves[0].app.vault.ramdiskAutostart.driveLetter),'Z');
    assert.equal(await page.evaluate(()=>window.__creates[1].DriveLetter),'Z');
    await page.getByRole('button',{name:'Create RAM Disk',exact:true}).click();
    await grid.waitFor();
    await dialog.getByRole('button',{name:'Refresh free letters',exact:true}).click();
    await page.waitForFunction(()=>window.__pendingLetters.length===1);
    await dialog.getByRole('status').filter({hasText:'Checking available drive letters'}).waitFor();
    assert.equal(await grid.getByRole('radio',{name:'J:',exact:true}).isDisabled(),true,'Refresh disables stale selections');
    await page.evaluate(()=>window.__resolveLetters({success:false,error:'unavailable'}));
    await dialog.getByRole('alert').filter({hasText:'could not be checked'}).waitFor();
    assert.equal(await dialog.getByText(/No free drive letters/).count(),0,'Read failure is not an exhausted list');
    assert.equal(await dialog.getByRole('button',{name:'CREATE',exact:true}).isDisabled(),true);
    await dialog.getByRole('button',{name:'CANCEL',exact:true}).click();
    await page.getByRole('region',{name:'Default dropdown'}).getByRole('combobox',{name:'Drive letter'}).waitFor();
    assert.equal(await page.getByRole('region',{name:'Default dropdown'}).getByRole('combobox',{name:'Drive letter'}).count(),1,'Other picker callers keep dropdowns');
    assert.deepEqual(errors,[]);
    console.log('PASS: Create and startup RAM disks use free-letter grids, cached startup values, keyboard selection, fresh create preflight, chosen save values, truthful refresh failures, and responsive widths; default picker remains dropdown.');
  } catch(error) {console.error({errors,body:await page.locator('body').innerText()});throw error}
  finally {await browser.close()}
})().catch(error=>{console.error(error);process.exitCode=1});
