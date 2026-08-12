#!/usr/bin/env bash
# CI guardrail (audit C1/H3): fail if a PowerShell `-Command` string is built by
# `format!` with an interpolation placeholder that is NOT routed through the
# `ps_single_quote` escaping helper (or the out-of-band env-var param path).
# Static `-Command` args (build_powershell_command, WMI/CIM probes) don't match
# because they use `.args([...])` with literal strings and no `{` placeholder.
set -euo pipefail
cd "$(dirname "$0")/../.."

hits=$(grep -rnE 'format!\([^)]*-Command[^)]*\{' --include='*.rs' src-tauri 2>/dev/null \
  | grep -vE 'ps_single_quote|WINCMD_PARAMS_JSON' || true)

if [ -n "$hits" ]; then
  echo "FAIL: a PowerShell -Command string interpolates a value without ps_single_quote:"
  echo "$hits"
  echo ""
  echo "Fix: wrap every interpolated untrusted value in ps_single_quote(...), or"
  echo "pass data out-of-band via \$env:WINCMD_PARAMS_JSON (see router.ps1)."
  exit 1
fi
echo "ps-command-quoting gate: clean"
