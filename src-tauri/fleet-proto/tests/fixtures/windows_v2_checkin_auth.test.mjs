// SPDX-License-Identifier: AGPL-3.0-or-later
import { createHmac } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { strict as assert } from 'node:assert';

const fixture = JSON.parse(readFileSync(new URL('./windows_v2_checkin_auth_vectors.json', import.meta.url), 'utf8'));
const secret = Buffer.from(fixture.secret_hex, 'hex');
const domain = Buffer.from('fleet.windows.checkin-auth.v2\nPOST\n/v1/agents/checkin\n', 'ascii');
const header = body => 'v2.' + createHmac('sha256', secret).update(domain).update(body).digest('base64');

test('independent Node HMAC verifies exact Unicode, sentinel and whitespace vectors', () => {
  for (const vector of fixture.vectors) assert.equal(header(Buffer.from(vector.body, 'utf8')), vector.header);
  assert.notEqual(fixture.vectors[1].header, fixture.vectors[2].header);
});

test('independent Node HMAC verifies the full 2 MiB body', () => {
  assert.equal(header(Buffer.alloc(2 * 1024 * 1024, 'x')), fixture.large_header);
});
