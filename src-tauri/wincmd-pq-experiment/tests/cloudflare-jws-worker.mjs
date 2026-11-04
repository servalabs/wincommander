// SPDX-License-Identifier: AGPL-3.0-or-later
// Disposable local-only fixture generator. Do not deploy.
// Generated with Wrangler 4.156.0, compatibility date 2026-10-01 and
// webcrypto_modern_algorithms. No private key is exported or retained.
export default { async fetch() {
  const ed = await crypto.subtle.generateKey('Ed25519', true, ['sign', 'verify']);
  const pq = await crypto.subtle.generateKey('ML-DSA-65', true, ['sign', 'verify']);
  const encode = new TextEncoder();
  const b64 = bytes => btoa(String.fromCharCode(...new Uint8Array(bytes))).replaceAll('+','-').replaceAll('/','_').replaceAll('=','');
  const payload = b64(encode.encode('disposable licence fixture'));
  const header = (alg, kid) => b64(encode.encode(JSON.stringify({alg, kid, typ:'application/wincmd-experimental+jws', crit:['profile','purpose','epoch'], profile:'experimental-ed25519-mldsa65-v1', purpose:'licence', epoch:7})));
  const edHeader = header('Ed25519','ed-key');
  const pqHeader = header('ML-DSA-65','pq-key');
  const edSig = await crypto.subtle.sign('Ed25519', ed.privateKey, encode.encode(`${edHeader}.${payload}`));
  const pqSig = await crypto.subtle.sign('ML-DSA-65', pq.privateKey, encode.encode(`${pqHeader}.${payload}`));
  const publicEd = b64(await crypto.subtle.exportKey('raw', ed.publicKey));
  const publicPq = (await crypto.subtle.exportKey('jwk', pq.publicKey)).pub;
  return Response.json({publicEd, publicPq, jws:{payload,signatures:[{protected:edHeader,signature:b64(edSig)},{protected:pqHeader,signature:b64(pqSig)}]}});
} };
