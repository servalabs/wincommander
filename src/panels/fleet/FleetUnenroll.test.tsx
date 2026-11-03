import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void };
};

test("unenroll requires an explicit approved response and a successful final recheck", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url:'http://localhost' });
    globalThis.window=dom.window; globalThis.document=dom.window.document;
    globalThis.navigator=dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT=true;
    const React=await import('react'); const {act,createElement:h}=React;
    const {createRoot}=await import('react-dom/client');
    const timers=new Map(); let nextTimer=0;
    globalThis.setInterval=(callback,delay)=>{const id=++nextTimer;timers.set(id,{callback,delay});return id;};
    globalThis.clearInterval=id=>timers.delete(id);
    let pollResult='unauthorized'; let requestFails=false; let disconnectFails=false; let disconnectMalformed=false; let disconnectCalls=0; let partial=false; let sharedFallback=false; let resolveStale;
    const status={connected:true,deviceId:'device-123',serverUrl:'https://fleet.test',lastEnrollAt:null,lastError:null,retrying:true};
    mock.module('@tauri-apps/api/core',()=>({invoke:async command=>{
      if(command==='fleet_status') return status;
      if(command==='fleet_machine_trust_status') return {trustBound:true,shieldPolicyStatus:'unverified'};
      if(command==='fleet_service_contact_status') return {observation:null};
      if(command==='fleet_request_unenroll') { if(requestFails) throw new Error('HTTP 401'); return {status:'pending',approvals:0,required_approvals:2}; }
      if(command==='fleet_unenroll_status') {
        if(pollResult==='unauthorized') throw new Error('unenroll-status: HTTP 401');
        if(pollResult==='synthetic') return {approved:true,status:'revoked'};
        if(pollResult==='malformed') return {approved:false,status:'pending'};
        if(pollResult==='deferred') return new Promise(resolve=>{resolveStale=resolve;});
        if(pollResult==='pending') return {approved:false,status:'pending',approvals:0,required_approvals:2};
        return {approved:true,status:'approved',approvals:0,required_approvals:0};
      }
      if(command==='fleet_disconnect') { disconnectCalls++; if(disconnectFails) throw new Error('HTTP 401'); if(sharedFallback) return {ok:false,agentStopped:true,sharedCleanupUnconfirmed:true,cleanupRetryable:true}; return {ok:!disconnectMalformed,serviceReachabilityStopped:!partial,cleanupRetryable:partial}; }
      throw new Error('unexpected command: '+command);
    }}));
    mock.module('@/hooks/useSettings',()=>({getSettingsOnce:async()=>({deviceId:'device-123',app:{fleet:{enabled:true}}})}));
    mock.module('@/hooks/queries/useLicenseQuery',()=>({useLicenseQuery:()=>({data:{valid:true,features:['fleet']},isLoading:false})}));
    mock.module('@/components/ui/button',()=>({Button:({children,variant,size,...props})=>h('button',props,children)}));
    mock.module('@/components/ui/input',()=>({Input:props=>h('input',props)}));
    mock.module('@/components/ui/switch',()=>({Switch:({checked,onCheckedChange,...props})=>h('input',{...props,type:'checkbox',checked,onChange:event=>onCheckedChange(event.target.checked)})}));
    mock.module('@/components/ui/icon',()=>({Icon:()=>null}));
    const {default:View}=await import('./src/panels/fleet/FleetConnectView');
    const host=document.getElementById('root'),root=createRoot(host);
    const leave=()=>[...host.querySelectorAll('button')].find(button=>button.textContent==='Request to leave');
    async function request(){await act(async()=>{leave().click();});assert.match(host.textContent,/waiting for admin approval/);}
    async function poll(){const timer=[...timers.values()].find(t=>t.delay===5000);assert.ok(timer);await act(async()=>{await timer.callback();});}
    await act(async()=>{root.render(h(View));});
    await request(); await poll();
    assert.match(host.textContent,/Could not verify Fleet removal status/);
    assert.doesNotMatch(host.textContent,/waiting for admin approval/);
    assert.equal(disconnectCalls,0);
    assert.match(host.querySelector('.fleet-connect-badge').textContent,/Linked/);
    pollResult='pending'; await poll();
    assert.match(host.textContent,/waiting for admin approval/);
    assert.doesNotMatch(host.textContent,/Could not verify Fleet removal status/);
    pollResult='unauthorized'; await poll();
    await request(); requestFails=true;
    const failedRefresh=[...host.querySelectorAll('button')].find(button=>button.textContent==='Refresh status');
    await act(async()=>{failedRefresh.click();}); requestFails=false;
    assert.doesNotMatch(host.textContent,/waiting for admin approval/);
    assert.match(host.textContent,/Could not verify Fleet removal status/);
    assert.equal(disconnectCalls,0);
    pollResult='deferred'; await request();
    let oldPoll;
    await act(async()=>{oldPoll=[...timers.values()].find(t=>t.delay===5000).callback();});
    const refresh=[...host.querySelectorAll('button')].find(button=>button.textContent==='Refresh status');
    await act(async()=>{refresh.click();});
    pollResult='unauthorized'; await poll();
    await act(async()=>{resolveStale({approved:true,status:'approved'});await oldPoll;});
    assert.equal(disconnectCalls,0,'a stale approval must not override a newer unconfirmed result');
    assert.match(host.textContent,/Could not verify Fleet removal status/);
    pollResult='synthetic'; await request(); await poll();
    assert.match(host.textContent,/Could not verify Fleet removal status/);
    assert.equal(disconnectCalls,0);
    pollResult='malformed'; await request(); await poll();
    assert.match(host.textContent,/Could not verify Fleet removal status/);
    assert.doesNotMatch(host.textContent,/waiting for admin approval/);
    assert.equal(disconnectCalls,0);
    await poll(); await poll();
    assert.ok(![...timers.values()].some(t=>t.delay===5000),'three malformed polls must stop polling');
    pollResult='approved'; disconnectFails=true; await request(); await poll();
    assert.equal(disconnectCalls,1);
    assert.match(host.textContent,/Could not verify Fleet removal status/);
    assert.doesNotMatch(host.textContent,/device was removed from Fleet/);
    assert.match(host.querySelector('.fleet-connect-badge').textContent,/Linked/);
    disconnectFails=false; disconnectMalformed=true; await request(); await poll();
    assert.equal(disconnectCalls,2);
    assert.match(host.textContent,/Could not verify Fleet removal status/);
    disconnectMalformed=false; sharedFallback=true; await request(); await poll();
    assert.equal(disconnectCalls,3);
    assert.match(host.textContent,/shared settings and service cleanup could not be confirmed/);
    assert.doesNotMatch(host.textContent,/device was removed from Fleet/);
    sharedFallback=false;
    const retryFallback=[...host.querySelectorAll('button')].find(button=>button.textContent==='Retry local cleanup');
    partial=true; await act(async()=>{retryFallback.click();});
    assert.equal(disconnectCalls,4);
    assert.doesNotMatch(host.textContent,/Could not verify Fleet removal status/);
    assert.match(host.textContent,/stopping local service reachability could not be confirmed/);
    const retry=[...host.querySelectorAll('button')].find(button=>button.textContent==='Retry local cleanup');
    assert.ok(retry); partial=false;
    await act(async()=>{retry.click();});
    assert.equal(disconnectCalls,5);
    assert.ok(![...host.querySelectorAll('button')].some(button=>button.textContent==='Retry local cleanup'));
    status.cleanupPending=true;
    await act(async()=>{root.unmount();});
    const remount=createRoot(host);
    await act(async()=>{remount.render(h(View));});
    assert.ok([...host.querySelectorAll('button')].some(button=>button.textContent==='Retry local cleanup'), 'an in-memory cleanup survives panel navigation');
    await act(async()=>{remount.unmount();});dom.window.close();
  `;
  const process=Bun.spawn([Bun.which("bun")!,"-e",script],{stdout:"pipe",stderr:"pipe"});
  let timedOut=false;
  const timer=setTimeout(()=>{timedOut=true;process.kill();},30_000);
  try {
    const stderr=await new Response(process.stderr).text();
    expect({code:await process.exited,stderr,timedOut}).toEqual({code:0,stderr:"",timedOut:false});
  } finally { clearTimeout(timer); }
},45_000);
