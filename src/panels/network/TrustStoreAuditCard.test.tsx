import { describe, expect, test } from "bun:test";
import { distinctIssuer } from "./TrustStoreAuditCard";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: 'pipe'; stderr: 'pipe' }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number> };
};

describe("trust store certificate issuer", () => {
  test("does not repeat a self-issued distinguished name", () => {
    const name = 'OU="NO LIABILITY ACCEPTED, Inc.", O=Example Trust';
    expect(distinctIssuer(name, name)).toBeNull();
    expect(distinctIssuer(name, ` ${name} `)).toBeNull();
  });

  test("retains a different issuer and suppresses an empty one", () => {
    expect(distinctIssuer("CN=Issued root", "CN=Issuer")).toBe("CN=Issuer");
    expect(distinctIssuer("CN=Issued root", " ")).toBeNull();
  });
});

test("trust audit shows self-issued names once and keeps a different issuer", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const React = await import('react');
    const { renderToStaticMarkup } = await import('react-dom/server');
    const certificate = (thumbprint, subject, issuer) => ({ scope: 'LocalMachine', store: 'Root', thumbprint, subject, issuer, serialNumber: '', notBefore: '', notAfter: '2030-01-01', signatureAlgorithm: '', publicKeyAlgorithm: '', hasPrivateKey: false, inWindowsAuthRoot: false });
    mock.module('@/hooks/useTrustStoreAudit', () => ({ useTrustStoreAudit: () => ({ busy: false, error: null, inspect() {}, audit: { referenceAvailable: true, certificates: [certificate('A', 'OU=Distinctive root, O=Example', 'OU=Distinctive root, O=Example'), certificate('B', 'CN=Issued root', 'CN=Different issuer')] } }) }));
    const { TrustStoreAuditCard } = await import('./src/panels/network/TrustStoreAuditCard');
    const html = renderToStaticMarkup(React.createElement(TrustStoreAuditCard));
    const text = new JSDOM(html).window.document.body.textContent;
    assert.equal(text.split('OU=Distinctive root, O=Example').length - 1, 1);
    assert.match(text, /Issuer: CN=Different issuer/);
  `;
  const child = Bun.spawn([Bun.which('bun') ?? 'bun', '-e', script], { stdout: 'pipe', stderr: 'pipe' });
  const [status, stderr] = await Promise.all([child.exited, new Response(child.stderr).text()]);
  expect(stderr || 'no errors').toBe('no errors');
  expect(status).toBe(0);
});
