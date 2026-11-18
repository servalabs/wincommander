#!/usr/bin/env bash
# CI guardrail: the DESTRUCTIVE_COMMANDS registry in authz/dispatch.rs must list
# exactly the commands in EXPECTED below. The list is extracted from the source
# and compared in both directions, so adding or removing a destructive command
# requires updating this script as well (the authorization/confinement review
# trigger). Runtime coverage is asserted separately by the Rust unit test
# `authz::tests::registry_covers_known_catastrophic_commands`.
set -euo pipefail
cd "$(dirname "$0")/../.."

REG="${1:-src-tauri/commander-free/src/authz/dispatch.rs}"
EXPECTED=(
  bluetooth_control_set
  delete_decoy
  disk_delete_item
  erase_encrypted_container
  fleet_connect
  fleet_repair_machine_trust
  fleet_set_dispatch
  full_lockdown
  internet_kill_switch_set
  lockdown
  run_destruct_step
  search_shred_direct
  secure_shred
)

# Names inside `pub const DESTRUCTIVE_COMMANDS ... = &[ ( "name", ... ), ];`
actual=$(awk '
  /pub const DESTRUCTIVE_COMMANDS/ { inreg = 1; next }
  inreg && /^\];/ { exit }
  inreg && match($0, /\("[a-z0-9_]+"/) { print substr($0, RSTART + 2, RLENGTH - 3) }
' "$REG" | tr -d '\r' | sort)
expected=$(printf '%s\n' "${EXPECTED[@]}" | sort)

if [ -z "$actual" ]; then
  echo "FAIL: could not extract DESTRUCTIVE_COMMANDS from $REG"
  exit 1
fi

removed=$(comm -13 <(echo "$actual") <(echo "$expected") || true)
added=$(comm -23 <(echo "$actual") <(echo "$expected") || true)
dupes=$(echo "$actual" | uniq -d || true)

fail=0
if [ -n "$removed" ]; then
  echo "FAIL: destructive command(s) missing from DESTRUCTIVE_COMMANDS ($REG):"
  echo "$removed" | sed 's/^/  - /'
  fail=1
fi
if [ -n "$added" ]; then
  echo "FAIL: command(s) in DESTRUCTIVE_COMMANDS but not in EXPECTED of this script:"
  echo "$added" | sed 's/^/  + /'
  fail=1
fi
if [ -n "$dupes" ]; then
  echo "FAIL: duplicate registry entries:"
  echo "$dupes" | sed 's/^/  ! /'
  fail=1
fi
if [ "$fail" -ne 0 ]; then
  echo ""
  echo "Every destructive/irreversible command must be registered in DESTRUCTIVE_COMMANDS and"
  echo "listed in EXPECTED here, so the change gets an authorization review."
  exit 1
fi
echo "destructive-authz gate: registry matches expected list ($(echo "$actual" | wc -l | tr -d ' ') commands)"
