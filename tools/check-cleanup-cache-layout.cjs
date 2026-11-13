// Browser regression for long App Launch Cache fields in the real details UI.
// Start the local Vite server first; no native IPC or user data is involved.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw new Error('A loopback Vite server is required');
const html = `<!doctype html><html><head><script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window); window.$RefreshReg$=()=>{}; window.$RefreshSig$=()=>type=>type;
window.__vite_plugin_react_preamble_installed__=true;
</script></head><body><div id="fixture"></div><script type="module" src="/__cache_layout_fixture.js"></script></body></html>`;
const moduleSource = `
import React from 'REACT_MODULE';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import '/src/components/shared/TraceDetailDialog.css';
import '/src/panels/cleanup/cleanupDisplay.css';
import CleanupDisplayDetails from '/src/components/cleanup/CleanupDisplayDetails.tsx';
const sample = Array.from({length:50}, (_,i) => ({id:'record-'+i,name:'Example '+i,path:'C:\\\\test\\\\example.exe',registryValues:{LongValue:'x'.repeat(10000)}}));
ReactDOM.createRoot(document.getElementById('fixture')).render(React.createElement(CleanupDisplayDetails,{category:{id:'amcache',label:'App Launch Cache',description:'Windows app launch records',icon:'history',severity:'danger'},mode:'advanced',count:50,items:[],rawData:{source:'snapshot',total:50,entries:[{category:'InventoryApplication',count:50,sample}]},clearing:false,clearLabel:'Clear',onClose:()=>{}}));
`;
async function main() {
  const browser = await chromium.launch({ headless:true, ...(process.env.WINCOMMANDER_PLAYWRIGHT_EXECUTABLE_PATH ? { executablePath:process.env.WINCOMMANDER_PLAYWRIGHT_EXECUTABLE_PATH } : {}) });
  const page = await browser.newPage({ viewport:{width:1280,height:800} });
  try {
    const source = await (await page.request.get(new URL('/src/components/cleanup/CleanupDisplayDetails.tsx',origin).href)).text();
    const reactModule = source.match(/from "([^\"]*\/react\.js[^\"]*)"/)?.[1];
    assert.ok(reactModule, 'Resolve the same React instance as the component');
    await page.route('**/__cache_layout_fixture.js', route => route.fulfill({contentType:'application/javascript',body:moduleSource.replace('REACT_MODULE',reactModule)}));
    await page.route('**/__cache_layout_fixture__', route => route.fulfill({contentType:'text/html',body:html}));
    await page.goto(new URL('/__cache_layout_fixture__',origin).href);
    const body = page.locator('.cleanup-readable-detail__body');
    const header = page.getByRole('columnheader',{name:'Name',exact:true});
    const details = page.getByRole('columnheader',{name:'Location',exact:true});
    await header.waitFor();
    const frame = await body.boundingBox();
    for (const element of [header,details,page.getByRole('searchbox',{name:'Filter details'}),page.getByRole('button',{name:'Next',exact:true})]) {
      const box = await element.boundingBox();
      assert.ok(frame && box && box.x >= frame.x && box.x + box.width <= frame.x + frame.width + 1, 'Control must remain inside the scrollable details area');
    }
    await page.getByRole('button',{name:'Next',exact:true}).click();
    assert.ok(await page.getByText('Example 10',{exact:true}).first().isVisible());
    console.log('Passed: long registry fields preserve visible cache columns, search, and paging.');
  } finally { await browser.close(); }
}
main().catch(error => { console.error(error); process.exitCode=1; });
