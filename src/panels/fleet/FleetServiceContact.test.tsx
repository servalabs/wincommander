import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void;
  };
};

test("service contact is historical, endpoint-bound, and unavailable on errors", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', {url:'http://localhost'});
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const {act,createElement:h}=React;
    const {createRoot}=await import('react-dom/client');
    let resolveOld;
    mock.module('@tauri-apps/api/core', () => ({invoke: async (_name, {deviceId}) => {
      if (deviceId === 'old') return new Promise(resolve => { resolveOld = resolve; });
      if (deviceId === 'good') return {observation:{observed_unix_ms:1760000000000}};
      throw new Error('service offline');
    }}));
    const {FleetServiceContact:Card}=await import('./src/panels/fleet/FleetServiceContact');
    const host=document.getElementById('root'); const root=createRoot(host);
    await act(async()=>{root.render(h(Card,{deviceId:'old'}));});
    assert.match(host.textContent,/unconfirmed/);
    await act(async()=>{root.render(h(Card,{deviceId:'good'}));});
    assert.match(host.textContent,/Last service-verified contact for this machine's Fleet service trust/);
    assert.match(host.textContent,/Historical device time/);
    assert.match(host.textContent,/can differ from the current Fleet connection/);
    assert.match(host.textContent,/Automatic silence protection remains unavailable/);
    // The same device can be enrolled into a different org or generation while
    // the independently held service trust still retains its older history.
    const view = (org, generation) => h('div', null, h('p', null, org + '/' + generation), h(Card, {deviceId:'good'}));
    await act(async()=>{root.render(view('org-a','generation-a'));});
    await act(async()=>{root.render(view('org-b','generation-b'));});
    assert.match(host.textContent,/org-b\/generation-b/);
    assert.match(host.textContent,/service's enrolled trust/);
    assert.match(host.textContent,/can differ from the current Fleet connection/);
    await act(async()=>{resolveOld({observation:{observed_unix_ms:1700000000000}});});
    assert.equal(host.querySelector('time').dateTime,new Date(1760000000000).toISOString());
    await act(async()=>{root.render(h(Card,{deviceId:'failed'}));});
    assert.match(host.textContent,/unconfirmed/);
    assert.equal(host.querySelector('time'),null);
    await act(async()=>{root.unmount();}); dom.window.close();
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; process.kill(); }, 30_000);
  try {
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr, timedOut }).toEqual({ code: 0, stderr: "", timedOut: false });
  } finally { clearTimeout(timer); }
}, 45_000);
