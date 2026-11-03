// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { createHash, createPrivateKey, createPublicKey, sign, verify } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";

const fixture = JSON.parse(readFileSync(new URL("./windows_v2.json", import.meta.url), "utf8"));
// Opaque JSON text remains a string: never parse it to rebuild signing bytes.
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}
const bytes = value => Buffer.from(canonical(value), "utf8");
const hash = value => createHash("sha256").update(value).digest();
const publicKey = base64 => createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), Buffer.from(base64, "base64")]), format: "der", type: "spki" });
// Public, deterministic fixture seeds; never production credentials.
const privateKey = seed => createPrivateKey({ key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), Buffer.alloc(32, seed)]), format: "der", type: "pkcs8" });

function commandBytes(command) {
  return bytes(["fleet.windows.command.v2", command.binding, command.idempotency_key, command.catalog_id, command.action_class, command.payload, command.epoch_version]);
}

test("Python, Rust and JavaScript agree on exact command/outcome/receipt signing bytes", () => {
  const command = JSON.parse(JSON.stringify(fixture.command));
  assert.equal(typeof command.epoch_version, "string");
  assert.equal(typeof command.payload, "string");
  assert.match(command.payload, /12345678901234567890123456789012345678901234567890/);
  assert.match(command.payload, /1e400/);
  assert.match(command.payload, /🔒/);
  assert.match(command.payload, /\\n/);
  const message = commandBytes(command);
  assert.equal(message.toString("utf8"), fixture.command_preimage);
  assert.equal(sign(null, message, privateKey(13)).toString("base64"), command.signature);
  assert.ok(verify(null, message, publicKey(command.signer_key), Buffer.from(command.signature, "base64")));
  const outcome = JSON.parse(JSON.stringify(fixture.outcome));
  const outcomeBytes = bytes(["fleet.windows.action-outcome.v2", outcome.version, outcome.binding, outcome.receipt_id, outcome.sequence, outcome.previous_hash, outcome.command_id, outcome.catalog_id, outcome.action_class, outcome.outcome, outcome.failure_stage, outcome.observed_at, outcome.result_digest]);
  assert.equal(outcomeBytes.toString("utf8"), fixture.outcome_preimage);
  assert.equal(hash(outcomeBytes).toString("hex"), outcome.record_hash);
  assert.equal(sign(null, hash(outcomeBytes), privateKey(7)).toString("base64"), outcome.signature);
  const proof = fixture.proof;
  const proofBytes = bytes(["fleet.windows.outcome-receipt.v2", proof.binding, proof.nonce, proof.request_sha256, proof.http_status, proof.response_sha256]);
  assert.equal(proofBytes.toString("utf8"), fixture.proof_preimage);
  assert.equal(hash(Buffer.from(fixture.request_bytes, "utf8")).toString("base64"), proof.request_sha256);
  assert.equal(hash(Buffer.from(fixture.response_bytes, "utf8")).toString("base64"), proof.response_sha256);
  assert.equal(sign(null, proofBytes, privateKey(11)).toString("base64"), proof.signature);
  assert.ok(verify(null, proofBytes, publicKey(proof.binding.authority_key_b64), Buffer.from(proof.signature, "base64")));
  assert.equal(hash(bytes(["fleet.windows.ack.v2", fixture.ack])).toString("hex"), fixture.ack_hash);
  assert.equal(hash(bytes(["fleet.windows.result.v2", fixture.ack.result])).toString("hex"), fixture.result_hash);
});

test("outer JSON roundtrip keeps full counters and payload spelling; altered text fails", () => {
  const command = JSON.parse(JSON.stringify({ ...fixture.command, epoch_version: "9223372036854775807" }));
  assert.equal(command.epoch_version, "9223372036854775807");
  const head = JSON.parse(JSON.stringify({ last_sequence: "18446744073709551615" }));
  assert.equal(head.last_sequence, "18446744073709551615");
  assert.equal(BigInt(head.last_sequence), 18446744073709551615n);
  const altered = { ...fixture.command, payload: ` ${fixture.command.payload}` };
  assert.ok(!verify(null, commandBytes(altered), publicKey(altered.signer_key), Buffer.from(altered.signature, "base64")));
});
