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
    for candidate in "$here"/*.pld; do basename "$candidate" .pld; done >&2
    exit 2
fi

if ! command -v wine >/dev/null 2>&1; then
    printf 'wine is not installed; this script is developer-only and CI does not run it\n' >&2
    exit 2
fi

work="$(mktemp -d)"
cp "$source_file" "$work/"

# Record what produced the result. A fuse mapping is only evidence if the
# thing that produced it can be identified later (SPEC.md §7.2).
#
# The hashes are taken of the executable and library that are ACTUALLY
# used, derived from the same variables the command line is built from.
# Hashing fixed paths instead meant `DECPLD_WIN_CUPL_EXE=...cupl40.exe`
# ran one binary and recorded the hash of another — a provenance record
# naming the wrong producer, which is worse than none, and precisely what
# §7.2 exists to prevent.
win_to_host() {
    # C:\Foo\Bar -> $WINEPREFIX/drive_c/Foo/Bar, case-insensitively on
    # the drive letter. Only drive C is mapped; anything else is a
    # configuration this script cannot verify, and it says so rather
    # than producing an unidentifiable record.
    local win="$1"
    case "$win" in
        [Cc]:\\*) printf '%s/drive_c/%s\n' "$DECPLD_WINEPREFIX" "$(printf '%s' "${win:3}" | tr '\\' '/')" ;;
        *) return 1 ;;
    esac
}

hash_of() {
    local host
    if ! host="$(win_to_host "$1")"; then
        printf 'cannot map %s to a host path; only drive C is handled\n' "$1" >&2
        exit 2
    fi
    if [[ ! -f "$host" ]]; then
        printf 'not found: %s (from %s)\n' "$host" "$1" >&2
        printf 'refusing to record a run whose producer cannot be identified\n' >&2
        exit 2
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$host" | cut -d' ' -f1
    else
        shasum -a 256 "$host" | cut -d' ' -f1
    fi
}

# WinCUPL's own version, rather than a hand-copied string. §7.2 lists it
# as a required field, and a version nobody captured cannot be checked
# against a run.
release_notes="$(win_to_host "${DECPLD_WIN_CUPL_ROOT}\\release_notes.txt" 2>/dev/null || true)"
wincupl_version="unknown"
if [[ -n "$release_notes" && -f "$release_notes" ]]; then
    # release_notes.txt is CRLF, and a stray \r is an invalid JSON
    # control character — it made the whole record unparseable while
    # looking fine to a human reading it.
    wincupl_version="$(sed -n 's/^Version:[[:space:]]*//p' "$release_notes" | head -1 | tr -d '\r\n')"
    wincupl_version="${wincupl_version:-unknown}"
fi

# The device type comes from the source, not from this script. Three of
# the experiments exist precisely to vary it (p22v10, g22v10, g22v10cp),
# and a hardcoded `g22v10` would have compiled them under the wrong one
# while still producing a plausible .jed.
# Some experiments measure a REFUSAL. Asking the oracle to use a supply
# rail as a signal is a real measurement -- it is how "not usable by a
# design" becomes a checked claim rather than a transcription of a
# pinout -- but its result is no JEDEC at all. Without a declared
# expectation those are indistinguishable from a broken wine setup, a
# missing device library, or a genuinely broken experiment, so a batch
# re-run of the suite would stop on one and a reader would reasonably
# conclude the suite was rotten.
#
# The declaration lives in the source, beside the reasoning, rather than
# in a list here that could drift away from it.
expect_refusal=no
# Matched on its own line, optionally after a comment opener, so the
# marker can sit inside a CUPL comment where the compiler ignores it.
# Prose mentioning the words mid-line does not match -- see
# marker-inert.pld, which says them inline and must still compile.
if grep -q '^[[:space:]]*\(/\*[[:space:]]*\)\?EXPECT[[:space:]]\{1,\}refusal[[:space:]]*$' "$source_file"; then
    expect_refusal=yes
fi

device="$(sed -n 's/^[[:space:]]*Device[[:space:]]\{1,\}\([A-Za-z0-9]\{1,\}\)[[:space:]]*;.*/\1/p' "$source_file" | head -1)"
if [[ -z "$device" ]]; then
    printf 'no Device declaration in %s\n' "$source_file" >&2
    exit 2
fi

# Backslashes are legion in Windows paths and are not valid JSON escapes,
# so the record has to escape them or it is not machine-readable.
json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# Built once and both run and recorded, so the record cannot describe a
# different invocation from the one that happened.
inner="set LIBCUPL=${DECPLD_WIN_CUPL_LIBRARY} && cd /d Z:${work//\//\\} && ${DECPLD_WIN_CUPL_EXE} -jaxfl ${device} ${experiment}"

cat > "$work/run.json" <<JSON
{
  "experiment": "${experiment}",
  "wine_version": "$(wine --version 2>/dev/null || echo unknown)",
  "wincupl_version": "${wincupl_version}",
  "device_type": "${device}",
  "cupl_exe_sha256": "$(hash_of "${DECPLD_WIN_CUPL_EXE}")",
  "device_library_sha256": "$(hash_of "${DECPLD_WIN_CUPL_LIBRARY}")",
  "command_line": "wine cmd /c \"$(json_escape "$inner")\"",
  "environment": { "LIBCUPL": "$(json_escape "${DECPLD_WIN_CUPL_LIBRARY}")" }
}
JSON

WINEPREFIX="$DECPLD_WINEPREFIX" wine cmd /c "$inner" >"$work/cupl.log" 2>&1 || true

if [[ -f "$work/${experiment}.jed" ]]; then
    if [[ "$expect_refusal" == yes ]]; then
        printf '%s declares EXPECT refusal but the oracle accepted it and produced a JEDEC.\n' \
            "$experiment" >&2
        printf 'That is a failed measurement, not a passing one: see %s\n' "$work" >&2
        exit 1
    fi
    printf '%s\n' "$work"
    exit 0
fi

if [[ "$expect_refusal" == yes ]]; then
    # The refusal IS the result. Report it on stdout so a batch run reads
    # as data rather than as breakage, and leave the log for inspection.
    printf 'refused as expected; see %s/cupl.log\n' "$work"
    exit 0
fi

printf 'no JEDEC produced; see %s/cupl.log\n' "$work" >&2
printf 'if this experiment is meant to be refused, declare "EXPECT refusal" in %s\n' \
    "$source_file" >&2
exit 1
