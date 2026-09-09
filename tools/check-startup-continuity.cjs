const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
(async () => {
const browser = await chromium.launch({headless:true});
try {
const fixture = '<html><head><script type="module">import RefreshRuntime from "/@react-refresh"; RefreshRuntime.injectIntoGlobalHook(window); window.$RefreshReg$=()=>{}; window.$RefreshSig$=()=>type=>type; window.__vite_plugin_react_preamble_installed__=true;</script></head><body></body></html>';
const page = await browser.newPage();
await page.route('**/__splash_continuity__', route => route.fulfill({contentType:'text/html', body:fixture}));
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
const delayedPage = await browser.newPage();
await delayedPage.route('**/__splash_continuity__', route => route.fulfill({contentType:'text/html', body:fixture}));
await delayedPage.goto(new URL('/__splash_continuity__', process.argv[2] || 'http://127.0.0.1:1435').href);
await delayedPage.evaluate(async () => {
 window.completed = 0;
 window.host = await import('/src/startup/animationRoot.tsx');
 window.props = { branding:{companyLabel:'SERVALABS',productLabel:'WINCOMMANDER'},
  isLight:false,reducedMotion:false,isWindowVisible:false,isAppReady:true,startupError:null,
  onComplete(){window.completed++;window.completedAt=performance.now()},onRetry(){} };
 window.host.showStartupAnimation(window.props);
 await window.host.waitForStartupAnimationReady();
 const shadow = document.querySelector('#startup-animation').shadowRoot;
 window.canvas = shadow.querySelector('canvas');
 window.ringAnimation = shadow.querySelector('.sp-ring-outer').getAnimations()[0];
 window.pausedTime = window.ringAnimation.currentTime;
});
// Longer than the full nine-letter intro (2160ms scramble + 400ms hold).
await delayedPage.waitForTimeout(3000);
const hidden = await delayedPage.evaluate(() => ({
 completed:window.completed, paused:window.ringAnimation.playState==='paused',
 timeUnchanged:window.ringAnimation.currentTime===window.pausedTime,
}));
assert.deepEqual(hidden,{completed:0,paused:true,timeUnchanged:true});
await delayedPage.evaluate(() => {
 window.revealedAt = performance.now();
 window.host.showStartupAnimation({...window.props,isWindowVisible:true});
});
await delayedPage.waitForTimeout(500);
const revealed = await delayedPage.evaluate(() => ({
 completed:window.completed, running:window.ringAnimation.playState==='running',
 timeAdvanced:window.ringAnimation.currentTime>window.pausedTime,
 sameCanvas:window.canvas===document.querySelector('#startup-animation').shadowRoot.querySelector('canvas'),
}));
assert.deepEqual(revealed,{completed:0,running:true,timeAdvanced:true,sameCanvas:true});
await delayedPage.waitForFunction(() => window.completed === 1,{},{timeout:5000});
await delayedPage.evaluate(() => window.host.showStartupAnimation({...window.props,isWindowVisible:true}));
await delayedPage.waitForTimeout(180);
const completion = await delayedPage.evaluate(() => ({
 count:window.completed, visibleDuration:window.completedAt-window.revealedAt,
}));
assert.equal(completion.count,1);
assert.ok(completion.visibleDuration >= 2500,'The intro must run after the native window becomes visible');
console.log(JSON.stringify({hidden,revealed,completion}));
} finally { await browser.close(); }
})().catch(e=>{console.error(e);process.exit(1)});
