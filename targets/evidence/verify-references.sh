#!/usr/bin/env bash
# Re-fetch every reference in references.toml and check it against the
# recorded sha256.
#
# This is deliberately NOT run by CI: it hits the network and depends on
# third-party hosting staying up, so a vendor reorganising their site
# would turn the build red for a reason unrelated to the code. Run it
# when you are about to rely on a document, or when a mapping derived
# from one is in question.
#
# A MISMATCH is not automatically a problem with deCPLD — it usually
# means the publisher reissued the document. It IS a signal to diff the
# new revision against the claims that cite it before trusting either.
#
#   targets/evidence/verify-references.sh              # check all
#   targets/evidence/verify-references.sh atf22v10c-datasheet
#
# Downloaded files land in a temp dir and are removed on exit; nothing
# third-party is written into the repository.
set -euo pipefail

cd "$(dirname "$0")"

manifest="references.toml"
want_id="${1:-}"

if ! command -v shasum >/dev/null 2>&1 && ! command -v sha256sum >/dev/null 2>&1; then
    printf "error: need shasum or sha256sum\n" >&2
    exit 1
fi

hash_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

# Pull id / url / sha256 triples out of the manifest. Entries without a
# sha256 (source repositories, which are cited by commit rather than by
# document hash) are reported as skipped rather than silently ignored —
# a quiet skip would read as "verified".
checked=0 failed=0 skipped=0
id="" url="" sha=""

flush() {
    [[ -n "${id}" ]] || return 0
    if [[ -n "${want_id}" && "${id}" != "${want_id}" ]]; then
        id="" url="" sha=""
        return 0
    fi
    if [[ -z "${sha}" ]]; then
        printf "SKIP    %-24s (no document hash; cite by commit)\n" "${id}"
        skipped=$((skipped + 1))
        id="" url="" sha=""
        return 0
    fi

    local out="${tmp}/${id}"
    local code
    code="$(curl -sS -L --max-time 120 -w '%{http_code}' -o "${out}" "${url}" 2>/dev/null || echo 000)"
    if [[ "${code}" != "200" ]]; then
        printf "FETCH   %-24s HTTP %s  %s\n" "${id}" "${code}" "${url}"
        failed=$((failed + 1))
    else
        local got
        got="$(hash_of "${out}")"
        if [[ "${got}" == "${sha}" ]]; then
            printf "ok      %-24s %s\n" "${id}" "${sha:0:16}…"
            checked=$((checked + 1))
        else
            printf "MISMATCH %-23s\n         recorded %s\n         fetched  %s\n" "${id}" "${sha}" "${got}"
            failed=$((failed + 1))
        fi
    fi
    id="" url="" sha=""
}

# Value of a `key = "value"` line: drop everything up to the opening
# quote, then everything from the closing quote on.
quoted_value() {
    local rest="${1#*\"}"
    printf '%s' "${rest%%\"*}"
}

while IFS= read -r line; do
    case "${line}" in
        '[[reference]]'*) flush ;;
        id\ =\ *)     id="$(quoted_value "${line}")" ;;
        url\ =\ *)    url="$(quoted_value "${line}")" ;;
        sha256\ =\ *) sha="$(quoted_value "${line}")" ;;
    esac
done < "${manifest}"
flush

printf "\n%d verified, %d skipped, %d failed\n" "${checked}" "${skipped}" "${failed}"
[[ "${failed}" -eq 0 ]]
