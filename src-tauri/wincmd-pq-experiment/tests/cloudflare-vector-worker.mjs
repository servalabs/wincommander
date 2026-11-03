// SPDX-License-Identifier: AGPL-3.0-or-later
// Disposable local-only fixture generator for cloudflare-hybrid-v1.bin.
// Do not deploy: every request generates fresh keys and exports public data.
// Generated with Wrangler 4.156.0, compatibility date 2026-10-01 and
// the webcrypto_modern_algorithms flag. No private key is exported or saved.
export default {
  async fetch() {
    const ed = await crypto.subtle.generateKey('Ed25519', true, ['sign', 'verify']);
    const pq = await crypto.subtle.generateKey('ML-DSA-65', true, ['sign', 'verify']);
    const payload = new TextEncoder().encode('WinCommander disposable interop probe');
    const domain = new TextEncoder().encode('WinCommander experimental hybrid signature v1\0');
    const body = new Uint8Array(52 + payload.length);
    body.set(new TextEncoder().encode('WCX1'), 0);
    body.set([1, 1, 2, 1], 4);
    new DataView(body.buffer).setBigUint64(8, 3n, false);
    body.fill(7, 16, 48);
    new DataView(body.buffer).setUint32(48, payload.length, false);
    body.set(payload, 52);
    const message = new Uint8Array(domain.length + body.length);
    message.set(domain);
    message.set(body, domain.length);
    const edSignature = new Uint8Array(await crypto.subtle.sign('Ed25519', ed.privateKey, message));
    const pqSignature = new Uint8Array(await crypto.subtle.sign('ML-DSA-65', pq.privateKey, message));
    const publicEd = new Uint8Array(await crypto.subtle.exportKey('raw', ed.publicKey));
    const publicPq = await crypto.subtle.exportKey('jwk', pq.publicKey);
    return Response.json({publicEd: Array.from(publicEd), publicPq: publicPq.pub, body: Array.from(body), edSignature: Array.from(edSignature), pqSignature: Array.from(pqSignature)});
  },
};
