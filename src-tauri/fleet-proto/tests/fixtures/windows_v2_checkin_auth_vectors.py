# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent stdlib HMAC oracle; synthetic fixture secret only."""
import base64
import hashlib
import hmac
import json
from pathlib import Path

secret = bytes(range(32))
domain = b"fleet.windows.checkin-auth.v2\nPOST\n/v1/agents/checkin\n"
sentinel = ('{"device_id":"Desk/λ", "ts":1767225600,"nonce":"0123456789abcdef",'
            '"telemetry":{"$serde_json::private::RawValue":"{\\"x\\":1}",'
            '"$serde_json::private::Number":"123"},"windows_v2":{"version":2}}')
bodies = ["", sentinel, sentinel + "\n"]
vectors = [{"body": body, "header": "v2." + base64.b64encode(
    hmac.new(secret, domain + body.encode("utf-8"), hashlib.sha256).digest()
).decode("ascii")} for body in bodies]
large_body = b"x" * (2 * 1024 * 1024)
large_header = "v2." + base64.b64encode(
    hmac.new(secret, domain + large_body, hashlib.sha256).digest()
).decode("ascii")
fixture = {"secret_hex": secret.hex(), "vectors": vectors, "large_header": large_header}
Path(__file__).with_suffix(".json").write_text(
    json.dumps(fixture, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
)
