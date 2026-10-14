import { describe, expect, test } from 'bun:test';
import { createDebouncedApply, type ApplyStatus } from './debouncedApply';

function fixture() {
  let now = 0, id = 0;
  const tasks = new Map<number, { at: number; fn: () => void; interval?: number }>();
  const clock = {
    setTimeout(fn: () => void, ms: number) { tasks.set(++id, { at: now + ms, fn }); return id; },
    setInterval(fn: () => void, ms: number) { tasks.set(++id, { at: now + ms, fn, interval: ms }); return id; },
    clearTimeout(key: number) { tasks.delete(key); },
    clearInterval(key: number) { tasks.delete(key); },
  } as unknown as typeof globalThis;
  const states: ApplyStatus[] = [];
  const controller = createDebouncedApply(5000, status => states.push(status), clock);
  async function advance(ms: number) {
    const end = now + ms;
    while (true) {
      const next = [...tasks].filter(([, task]) => task.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
      if (!next) break;
      const [key, task] = next;
      now = task.at;
      if (task.interval) task.at += task.interval; else tasks.delete(key);
      task.fn();
      await Promise.resolve(); await Promise.resolve();
    }
    now = end;
  }
  return { controller, states, advance };
}

describe('DNS apply receipt presentation', () => {
  test('only a true receipt becomes applied and later idle', async () => {
    const f = fixture(); f.controller.schedule(async () => true);
    await f.advance(5000); expect(f.states.at(-1)).toBe('applied');
    await f.advance(2000); expect(f.states.at(-1)).toBe('idle');
  });
  test('refusal and exceptions stay unconfirmed', async () => {
    for (const apply of [async () => false, async () => { throw new Error('denied'); }]) {
      const f = fixture(); f.controller.schedule(apply);
      await f.advance(10000); expect(f.states.at(-1)).toBe('failed');
      expect(f.states).not.toContain('applied');
    }
  });
  test('pending cancellation does not dispatch', async () => {
    const f = fixture(); let calls = 0;
    f.controller.schedule(async () => { calls++; return true; }); f.controller.cancel();
    await f.advance(10000); expect(calls).toBe(0); expect(f.states.at(-1)).toBe('idle');
  });
  test('cancelled in-flight work cannot claim cancellation or overwrite a newer failure', async () => {
    const f = fixture(); let finish!: (value: boolean) => void;
    f.controller.schedule(() => new Promise(resolve => { finish = resolve; }));
    await f.advance(5000); f.controller.cancel(); expect(f.states.at(-1)).toBe('unknown');
    f.controller.schedule(async () => false); await f.advance(5000);
    finish(true); await Promise.resolve(); await Promise.resolve();
    expect(f.states.at(-1)).toBe('failed'); expect(f.states).not.toContain('applied');
  });
  test('disposal ignores completion and cancels timers; a fresh lifetime works', async () => {
    const f = fixture(); let finish!: (value: boolean) => void;
    f.controller.schedule(() => new Promise(resolve => { finish = resolve; }));
    await f.advance(5000); f.controller.dispose(); const before = [...f.states];
    finish(true); await f.advance(10000); expect(f.states).toEqual(before);
    f.controller.schedule(async () => true); await f.advance(5000); expect(f.states).toEqual(before);
    const fresh = fixture(); fresh.controller.schedule(async () => true);
    await fresh.advance(5000); expect(fresh.states.at(-1)).toBe('applied');
  });
});
