#!/usr/bin/env bash
# CI guardrail: a PowerShell `-Command` argument must not be a script built with
# `format!` that interpolates runtime values, unless the values are made safe.
# Real call sites pass the script as the argument after `"-Command"`, either
# inline (`&format!(..)`) or through a variable defined by
# `let <var> = format!(..)`. Literal scripts (const / string literal) and
# `-Command -` (script on stdin) are not matched.
#
# A finding is accepted only when one of these holds:
#   * the format! expression uses an approved escaping helper (HELPERS below) or
#     reads parameters out-of-band through WINCMD_PARAMS_JSON; or
#   * the `let <var> = format!` line (or the line above it) carries
#     `// ps-quoting: allow(<reason>)`, or the site is listed in ALLOW below
#     (file-suffix|variable|reason), written only after every interpolated
#     value was checked to be a constant, a number, or already escaped.
#     ALLOW is keyed by variable, not line number, so it survives edits;
#     "<inline>" covers `&format!(..)` passed directly to -Command.
# Prefer passing data out-of-band ($env:WINCMD_PARAMS_JSON, see router.ps1).
#
# Usage: check-ps-command-quoting.sh [root-dir]   (default: src-tauri)
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT="${1:-src-tauri}"

# Approved escaping helpers; add the real name here when one is introduced.
HELPERS='ps_single_quote|ps_quote|escape_ps_single|WINCMD_PARAMS_JSON'

# Reviewed sites whose interpolated values are constants, numbers or escaped.
ALLOW=(
  "commander-free/src/attend_watch.rs|script|task name is a constant, task path a constant, args are a fixed flag list plus the ProgramData script path and a clamped integer; name is quote-escaped"
  "commander-free/src/backend.rs|script|pid is the current process id, exe_dir is quote-escaped, shutdown_cmd is a fixed literal"
  "commander-free/src/backend.rs|fixture|test-only; interpolates a bool"
  "commander-free/src/camera_presence.rs|script|interpolates a fixed true/false literal and compile-time script constants; test samples are literals"
  "commander-free/src/lib.rs|<inline>|task name derives from a compile-time app display name"
  "commander-free/src/pro_install.rs|<inline>|both values are quote-escaped with single quote doubled before formatting"
  "commander-free/src/rdp_redirection.rs|script|registry value names and values come from a fixed match on known capabilities; guard is a static str"
  "commander-svc/src/rdp_lock_query_tests.rs|script|test-only fixtures and a compile-time constant"
)

is_allowed() { # $1=file $2=var
  local entry
  for entry in "${ALLOW[@]}"; do
    case "$1" in *"${entry%%|*}") [ "$(echo "$entry" | cut -d'|' -f2)" = "$2" ] && return 0;; esac
  done
  return 1
}

scan_file() {
  awk -v helpers="$HELPERS" '
    function trim(s) { gsub(/^[ \t\r]+|[ \t\r]+$/, "", s); return s }
    function interpolates(b,   t) {
      t = b; gsub(/\{\{|\}\}/, "", t)
      return (t ~ /\{[A-Za-z0-9_:.#?<>, ]*\}/)
    }
    { L[NR] = $0 }
    END {
      for (i = 1; i <= NR; i++) {
        if (L[i] !~ /"-Command"/) continue
        rest = L[i]; sub(/.*"-Command"/, "", rest); sub(/^[ \t]*,?/, "", rest)
        t = trim(rest)
        for (j = 1; t == "" && j <= 3; j++) t = trim(L[i + j])
        if (t ~ /^&?format!\(/) {
          blk = ""
          for (k = i; k <= i + 15 && k <= NR; k++) blk = blk "\n" L[k]
          if (blk !~ helpers && blk !~ /ps-quoting: allow/ && interpolates(blk))
            print FILENAME "	" i "	<inline>	inline format! passed to -Command"
        } else if (match(t, /^&?[a-z_][a-z0-9_]*/)) {
          v = substr(t, RSTART, RLENGTH); sub(/^&/, "", v)
          cmd[v] = i
        }
      }
      for (v in cmd) {
        for (i = 1; i <= NR; i++) {
          if (L[i] !~ ("let (mut )?" v "(: *[A-Za-z]+)? *= *&?format!\\(")) continue
          blk = ""
          for (k = i; k <= i + 80 && k <= NR; k++) {
            blk = blk "\n" L[k]
            if (L[k] ~ /\);[ \t\r]*$/) break
          }
          if (blk ~ helpers) continue
          if (blk ~ /ps-quoting: allow/ || (i > 1 && L[i - 1] ~ /ps-quoting: allow/)) continue
          if (interpolates(blk))
            print FILENAME "	" i "	" v "	`" v "` built by format! with interpolation, passed to -Command"
        }
      }
    }' "$1"
}

hits=""
while IFS= read -r -d '' f; do
  grep -q '"-Command"' "$f" || continue
  while IFS=$'\t' read -r file line var msg <&3; do
    [ -z "${file:-}" ] && continue
    is_allowed "$file" "$var" && continue
    hits="${hits}${file}:${line}: ${msg}"$'\n'
  done 3< <(scan_file "$f")
done < <(find "$ROOT" -name '*.rs' -not -path '*/target/*' -print0)

if [ -n "$hits" ]; then
  echo "FAIL: a PowerShell -Command script is built by format! with interpolated values and no approved quoting:"
  printf '%s' "$hits"
  echo ""
  echo "Fix: escape each untrusted value with an approved helper, pass data out-of-band via"
  echo "\$env:WINCMD_PARAMS_JSON, or (only if every value is a constant/number/already escaped)"
  echo "add an ALLOW entry (file|variable|reason) in this script."
  exit 1
fi
echo "ps-command-quoting gate: clean"
