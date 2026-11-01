import { expect, test } from 'bun:test';

test('selected-account scheduling actions route and verify one account', async () => {
  // Keep the minimal mocked hook runtime isolated from the rest of the suite.
  const child = Bun.spawn([process.execPath, 'test', './tools/tests/cleanup-selected-account-hook.fixture.ts'], {
    cwd: process.cwd(), stdout: 'pipe', stderr: 'pipe',
  });
  const [code, stdout, stderr] = await Promise.all([child.exited, new Response(child.stdout).text(), new Response(child.stderr).text()]);
  expect(`${code}\n${stdout}\n${stderr}`).toContain('1 pass');
  expect(code).toBe(0);
}, 15_000);
