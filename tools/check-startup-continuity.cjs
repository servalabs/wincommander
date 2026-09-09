const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
(async () => {
const browser = await chromium.launch({headless:true});
try {
const page = await browser.newPage();
await page.route('**/__splash_continuity__', route => route.fulfill({contentType:'text/html', body:'<html><head><script type="module">import RefreshRuntime from "/@react-refresh"; RefreshRuntime.injectIntoGlobalHook(window); window.$RefreshReg$=()=>{}; window.$RefreshSig$=()=>type=>type; window.__vite_plugin_react_preamble_installed__=true;</script></head><body></body></html>'}));
await page.goto(new URL('/__splash_continuity__', process.argv[2] || 'http://127.0.0.1:1435').href);
await page.evaluate(async()=>{
 window.contextCalls=0; window.completed=0;
 const original=HTMLCanvasElement.prototype.getContext;
 HTMLCanvasElement.prototype.getContext=function(...args){window.contextCalls++; return original.apply(this,args)};
 window.host=await import('/src/startup/animationRoot.tsx');
 window.props={branding:{companyLabel:'SERVALABS',productLabel:'WINCOMMANDER'},isLight:false,reducedMotion:false,isAppReady:false,startupError:null,onComplete(){window.completed++},onRetry(){}};
 window.host.showStartupAnimation(window.props);
});
await page.waitForTimeout(3200);
const result = await page.evaluate(async()=>{
 const root=()=>document.querySelector('#startup-animation').shadowRoot;
 const wait=()=>new Promise(resolve=>setTimeout(resolve,180));
 const canvas=root().querySelector('canvas');
 const ring=root().querySelector('.sp-ring-outer');
 const animation=ring.getAnimations()[0];
 const time=animation.currentTime;
 const font=getComputedStyle(root().querySelector('.sp-brand')).fontFamily;
 window.host.hideStartupAnimation();
 window.host.showStartupAnimation({...window.props,isLight:true,reducedMotion:true,branding:{companyLabel:'NEW BRAND',productLabel:'APP'}});
 await import('/src/index.css'); await import('/src/styles/v2-theme.css');
 document.documentElement.classList.add('light','wc-no-motion');
 await wait();
 const continuity={sameCanvas:canvas===root().querySelector('canvas'),sameAnimation:animation===ring.getAnimations()[0],timeAdvanced:animation.currentTime>time,brand:root().querySelector('.sp-brand').textContent,sameFont:font===getComputedStyle(root().querySelector('.sp-brand')).fontFamily,canvasInitializations:window.contextCalls};
 window.host.showStartupAnimation({...window.props,startupError:'Settings unavailable'}); await wait();
 const paused=animation.playState==='paused';
 window.host.showStartupAnimation(window.props); await wait();
 const resumed=animation.playState==='running' && window.contextCalls===1;
 window.host.showStartupAnimation({...window.props,isAppReady:true}); await wait();
 window.host.showStartupAnimation({...window.props,isAppReady:true}); await wait();
 const completed=window.completed;
 window.host.hideStartupAnimation(); await wait();
 return {...continuity,paused,resumed,completed,dismissed:document.querySelector('#startup-animation').hidden && !root().querySelector('canvas')};
});
assert.deepEqual(result,{sameCanvas:true,sameAnimation:true,timeAdvanced:true,brand:'SERVALABS',sameFont:true,canvasInitializations:1,paused:true,resumed:true,completed:1,dismissed:true});
console.log(JSON.stringify(result));
} finally { await browser.close(); }
})().catch(e=>{console.error(e);process.exit(1)});
