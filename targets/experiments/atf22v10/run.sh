#!/usr/bin/env bash
# Re-run one ATF22V10 differential experiment through the WinCUPL oracle.
#
# THIS IS NOT RUN BY CI, AND MUST NEVER BE.
#
# CI verifies deCPLD against itself — encode/decode round-trips, region
# invariants, equivalence checks — none of which need an oracle. deCPLD's
# whole point is that compiling never requires WinCUPL, Wine, or Windows
# (CLAUDE.md), so a CI job that reached for them would contradict the
# product. This script exists for a human establishing or re-checking a
# fuse mapping, in the same spirit as `targets/evidence/verify-references.sh`.
#
# WinCUPL's OUTPUT IS NOT COMMITTED. The `.pld` files beside this script
# are ours and the command line is right here, so any claim in
# `targets/evidence/atf22v10-fuse-map.md` can be reproduced and audited
# without redistributing a proprietary artifact. What lands in the
# repository is the device model, with each constant citing the
# experiment that established it.
#
#   targets/experiments/atf22v10/run.sh in2
#   targets/experiments/atf22v10/run.sh arch-reg-low
#
# Output goes to a scratch directory, printed at the end. Point deCPLD's
# analysis at it from there.
set -euo pipefail

: "${DECPLD_WINEPREFIX:=$HOME/.wine}"
: "${DECPLD_WIN_CUPL_ROOT:=C:\\WinCUPL}"
: "${DECPLD_WIN_CUPL_EXE:=${DECPLD_WIN_CUPL_ROOT}\\Shared\\cupl.exe}"
: "${DECPLD_WIN_CUPL_LIBRARY:=${DECPLD_WIN_CUPL_ROOT}\\Shared\\cupl.dl}"

experiment="${1:?usage: run.sh <experiment-name>}"
here="$(cd "$(dirname "$0")" && pwd)"
source_file="$here/${experiment}.pld"

if [[ ! -f "$source_file" ]]; then
    printf 'no such experiment: %s\n' "$source_file" >&2
    printf 'available:\n' >&2
    ls "$here"/*.pld | xargs -n1 basename | sed 's/\.pld$/  /' >&2
    exit 2
fi

if ! command -v wine >/dev/null 2>&1; then
    printf 'wine is not installed; this script is developer-only and CI does not run it\n' >&2
    exit 2
fi

work="$(mktemp -d)"
cp "$source_file" "$work/"

# Record what produced the result. A fuse mapping is only evidence if the
# thing that produced it can be identified later (SPEC.md §5.9).
prefix_path="${DECPLD_WINEPREFIX}/drive_c/${DECPLD_WIN_CUPL_ROOT#C:\\}"
prefix_path="${prefix_path//\\//}"
hash_of() { [[ -f "$1" ]] && shasum -a 256 "$1" | cut -d' ' -f1 || echo "(absent)"; }

command_line="${DECPLD_WIN_CUPL_EXE} -jaxfl g22v10 ${experiment}"
cat > "$work/run.json" <<JSON
{
  "experiment": "${experiment}",
  "wine_version": "$(wine --version 2>/dev/null || echo unknown)",
  "cupl_exe_sha256": "$(hash_of "${prefix_path}/Shared/cupl.exe")",
  "device_library_sha256": "$(hash_of "${prefix_path}/Shared/cupl.dl")",
  "command_line": "${command_line}",
  "environment": { "LIBCUPL": "${DECPLD_WIN_CUPL_LIBRARY}" }
}
JSON

# The executable path is passed unquoted. SPEC.md §5.9's representative
# wrapper quotes it, but `wine cmd` receives the backslashes literally
# and reports "Can't recognize ... as an internal or external command".
# The installed path contains no spaces, so quoting buys nothing here —
# and §5.9 says to record what the installed executable actually does
# rather than assume every release behaves alike.
WINEPREFIX="$DECPLD_WINEPREFIX" wine cmd /c \
    "set LIBCUPL=${DECPLD_WIN_CUPL_LIBRARY} && cd /d Z:${work//\//\\} && ${DECPLD_WIN_CUPL_EXE} -jaxfl g22v10 ${experiment}" \
    >"$work/cupl.log" 2>&1 || true

if [[ ! -f "$work/${experiment}.jed" ]]; then
    printf 'no JEDEC produced; see %s/cupl.log\n' "$work" >&2
    exit 1
fi

printf '%s\n' "$work"
