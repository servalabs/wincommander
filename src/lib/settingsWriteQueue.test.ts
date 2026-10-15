import { describe, expect, test } from "bun:test";
import { enqueueSettingsWrite } from "./settingsWriteQueue";

describe("shared settings write ordering", () => {
  test("evaluates each update against the last accepted snapshot and publishes it immediately", async () => {
    const queue = { current: Promise.resolve() as Promise<unknown> };
    let latest: string[] = [];
    const publications: string[][] = [];
    let finish!: () => void;
    const first = enqueueSettingsWrite(queue, async () => {
      const next = [...latest, "first"];
      await new Promise<void>((resolve) => { finish = resolve; });
      latest = next;
      publications.push(next);
      return next;
    });
    const second = enqueueSettingsWrite(queue, async () => {
      const next = [...latest, "second"];
      latest = next;
      publications.push(next);
      return next;
    });
    await Promise.resolve();
    expect(publications).toHaveLength(0);
    finish();
    expect(await first).toEqual(["first"]);
    expect(await second).toEqual(["first", "second"]);
    expect(publications).toEqual([["first"], ["first", "second"]]);
  });

  test("retains the rejection and allows a later write without retrying the failed operation", async () => {
    const queue = { current: Promise.resolve() as Promise<unknown> };
    let attempts = 0;
    const failure = new Error("write refused");
    const rejected = enqueueSettingsWrite(queue, async () => { attempts++; throw failure; });
    const recovered = enqueueSettingsWrite(queue, async () => "accepted");
    expect(await rejected.catch(error => error)).toBe(failure);
    expect(await recovered).toBe("accepted");
    expect(attempts).toBe(1);
  });

  test("checks current authorization when queued work actually begins", async () => {
    const queue = { current: Promise.resolve() as Promise<unknown> };
    let allowed = true;
    let nativeWrites = 0;
    const pending = enqueueSettingsWrite(queue, async () => {
      if (!allowed) throw new Error("decoy");
      nativeWrites++;
    });
    allowed = false;
    expect((await pending.catch(error => error)).message).toBe("decoy");
    expect(nativeWrites).toBe(0);
  });

  test("a probe finishing behind queued preferences cannot overwrite them or run failure fallback", async () => {
    const queue = { current: Promise.resolve() as Promise<unknown> };
    let generation = 0, preference = false, seedWrites = 0, fallbacks = 0;
    let release!: () => void;
    const first = enqueueSettingsWrite(queue, async () => {
      generation++;
      await new Promise<void>(resolve => { release = resolve; });
    });
    await Promise.resolve();
    const probeGeneration = generation;
    const choice = enqueueSettingsWrite(queue, async () => { generation++; preference = true; });
    const probe = enqueueSettingsWrite(queue, async () => {
      seedWrites++; preference = false; fallbacks++; return 'admitted';
    }, { admit: () => generation === probeGeneration, skipped: () => 'skipped' });
    release(); await first; await choice;
    expect(await probe).toBe('skipped'); expect(preference).toBe(true);
    expect(seedWrites).toBe(0); expect(fallbacks).toBe(0);
  });

  test("a genuine seed fallback completes inside its slot before the next preference", async () => {
    const queue = { current: Promise.resolve() as Promise<unknown> };
    let value = 'cached';
    let release!: () => void;
    const seed = enqueueSettingsWrite(queue, async () => {
      try { await new Promise<void>(resolve => { release = resolve; }); throw new Error('denied'); }
      catch { value = 'session baseline'; }
    });
    const choice = enqueueSettingsWrite(queue, async () => { value = 'explicit preference'; });
    await Promise.resolve(); release(); await seed; await choice;
    expect(value).toBe('explicit preference');
  });
});
