// Render the real status component against controlled native read receipts.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw new Error('Fixture requires loopback');
const html = `<!doctype html><html><head><script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {}; window.$RefreshSig$ = () => type => type;
window.__vite_plugin_react_preamble_installed__ = true;
</script></head><body><div id="fixture"></div><script type="module" src="/__elevation_fixture.js"></script></body></html>`;
const native = `export async function invoke(command) {
  window.__commands.push(command);
  return new Promise((resolve,reject)=>window.__pending.push({resolve,reject}));
}`;
const fixture = `
import React from '__REACT_MODULE__';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import '/src/index.css';
import '/src/styles/v2-theme.css';
import '/src/panels/secret/index.css';
import Status from '/src/panels/secret/ProcessElevationStatus.tsx';
window.__commands=[];window.__pending=[];
function Fixture() {
  const [version,setVersion]=React.useState(0);
  const [visible,setVisible]=React.useState(true);
  window.__remount=()=>setVersion(value=>value+1);
  window.__visible=setVisible;
  return React.createElement('div',{style:{width:'540px',margin:'30px'}},visible && React.createElement(Status,{key:version}));
}
ReactDOM.createRoot(document.getElementById('fixture')).render(React.createElement(Fixture));
`;

async function main() {
  const browser = await chromium.launch({headless:true,executablePath:process.env.WINCOMMANDER_BROWSER_EXECUTABLE || undefined});
  const page = await browser.newPage({viewport:{width:1100,height:650}});
  const errors=[]; page.on('pageerror',error=>errors.push(error.message));
  page.setDefaultTimeout(15000);
  try {
    const source=await (await page.request.get(new URL('/src/hooks/useProcessElevation.ts',origin).href)).text();
    const reactModule=source.match(/from "([^"]*\/react\.js[^"]*)"/)?.[1];
    const nativeModule=source.match(/from "([^"]*tauri-apps_api_core[^"]*)"/)?.[1];
    assert.ok(reactModule);assert.ok(nativeModule);
    await page.route(new URL(nativeModule,origin).href,route=>route.fulfill({contentType:'application/javascript',body:native}));
    await page.route('**/__elevation_fixture.js',route=>route.fulfill({contentType:'application/javascript',body:fixture.replaceAll('__REACT_MODULE__',reactModule)}));
    await page.route('**/__elevation__',route=>route.fulfill({contentType:'text/html',body:html}));
    await page.goto(new URL('/__elevation__',origin).href);
    const status=page.getByRole('status');
    await page.getByText('Checking privileges…',{exact:true}).waitFor();
    assert.equal(await page.getByText('Standard (not elevated)',{exact:true}).count(),0);
    await page.waitForFunction(()=>window.__pending.length===1);
    await page.evaluate(()=>window.__pending.shift().resolve(true));
    await page.getByText('Administrator (elevated)',{exact:true}).waitFor();
    if (process.env.WINCOMMANDER_ELEVATION_SCREENSHOT) await page.screenshot({path:process.env.WINCOMMANDER_ELEVATION_SCREENSHOT,fullPage:true});
    await page.evaluate(()=>window.__remount());
    await page.waitForFunction(()=>window.__pending.length===1);
    await page.evaluate(()=>window.__pending.shift().resolve(false));
    await page.getByText('Standard (not elevated)',{exact:true}).waitFor();
    await page.evaluate(()=>window.__remount());
    await page.waitForFunction(()=>window.__pending.length===1);
    await page.evaluate(()=>window.__pending.shift().reject(new Error('native token read failed')));
    await page.getByText('Privileges unavailable',{exact:true}).waitFor();
    assert.equal(await page.getByText('Standard (not elevated)',{exact:true}).count(),0);
    await page.getByRole('button',{name:'Check again',exact:true}).click();
    await page.getByText('Checking privileges…',{exact:true}).waitFor();
    await page.waitForFunction(()=>window.__pending.length===1);
    await page.evaluate(()=>window.__pending.shift().resolve('true'));
    await page.getByText('Privileges unavailable',{exact:true}).waitFor();
    await page.getByRole('button',{name:'Check again',exact:true}).click();
    await page.waitForFunction(()=>window.__pending.length===1);
    await page.evaluate(()=>{window.__visible(false);});
    await status.waitFor({state:'hidden'});
    await page.evaluate(()=>{window.__pending.shift().resolve(true);window.__visible(true);});
    await page.getByText('Checking privileges…',{exact:true}).waitFor();
    await page.waitForFunction(()=>window.__pending.length===1);
    await page.evaluate(()=>window.__pending.shift().resolve(false));
    await page.getByText('Standard (not elevated)',{exact:true}).waitFor();
    assert.ok((await page.evaluate(()=>window.__commands)).every(command=>command==='is_current_process_elevated'));
    assert.deepEqual(errors,[]);
    console.log('PASS: actual-token status component distinguishes loading, elevated, standard, rejected and malformed reads; retry and unmount are safe. Native reads simulated.');
  } finally {await browser.close();}
}
main().catch(error=>{console.error(error);process.exitCode=1;});
