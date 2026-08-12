#!/usr/bin/env bash
# CI guardrail (audit C2/C4/H1/H2/L1): the DESTRUCTIVE_COMMANDS registry in
# authz.rs must list every catastrophic Tauri command, so a destructive command
# cannot silently ship without an entry (and the accompanying authorization /
# confinement review). Removing an entry fails the build here.
# The registry's runtime coverage is separately asserted by the Rust unit test
# `authz::tests::registry_covers_known_catastrophic_commands`.
set -euo pipefail
cd "$(dirname "$0")/../.."

REG=src-tauri/commander-free/src/authz.rs
EXPECTED=(
  lockdown
  full_lockdown
  run_destruct_step
  fleet_connect
  disk_delete_item
  delete_decoy
  internet_kill_switch_set
)

missing=0
for cmd in "${EXPECTED[@]}"; do
  if ! grep -q "\"$cmd\"" "$REG"; then
    echo "FAIL: destructive command '$cmd' missing from DESTRUCTIVE_COMMANDS ($REG)"
    missing=1
  fi
done

if [ "$missing" -ne 0 ]; then
  echo ""
  echo "Every destructive/irreversible command must be registered in"
  echo "DESTRUCTIVE_COMMANDS so it is covered by the authorization review."
  exit 1
fi
echo "destructive-authz gate: registry intact"
