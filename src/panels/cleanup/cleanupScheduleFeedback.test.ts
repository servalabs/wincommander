import { expect, test } from 'bun:test';

declare const Bun: {
  which(name: string): string | null;
  spawn(command: string[], options: { stdout: 'pipe'; stderr: 'pipe' }): {
    exited: Promise<number>; stdout: ReadableStream; stderr: ReadableStream;
  };
};

test('schedule notifications reflect only the verified requested operation', async () => {
  const child = Bun.spawn([Bun.which('bun')!, 'test', './tools/tests/cleanup-schedule-feedback.fixture.ts'], { stdout: 'pipe', stderr: 'pipe' });
  const [code, stdout, stderr] = await Promise.all([child.exited, new Response(child.stdout).text(), new Response(child.stderr).text()]);
  expect(`${stdout}\n${stderr}`).toContain('1 pass');
  expect(code).toBe(0);
}, 15_000);
