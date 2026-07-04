#!/usr/bin/env bash
set -euo pipefail

LIONSOS_COMMIT="${LIONSOS_COMMIT:-748ccb4a8cb3c836ab48161e44f9f1e788028520}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSIONS_MK="$REPO_ROOT/build-system/config/versions.mk"
UPSTREAM_URL="https://raw.githubusercontent.com/au-ts/lionsos/${LIONSOS_COMMIT}/flake.nix"

repo_version="$(sed -n 's/^MICROKIT_VERSION := //p' "$VERSIONS_MK")"
if [[ -z "$repo_version" ]]; then
    echo "unable to read MICROKIT_VERSION from $VERSIONS_MK" >&2
    exit 1
fi

flake="$(curl --fail --location --retry 3 --silent --show-error "$UPSTREAM_URL")"
upstream_version="$(printf '%s\n' "$flake" | sed -n 's/^[[:space:]]*microkit-version = "\([^"]*\)";.*/\1/p' | head -n1)"
sdfgen_version="$(printf '%s\n' "$flake" | sed -n 's/.*microkit_sdf_gen\/\([^"]*\)";.*/\1/p' | head -n1)"

if [[ -z "$upstream_version" ]]; then
    echo "unable to identify LionsOS Microkit pin at $LIONSOS_COMMIT" >&2
    exit 1
fi

printf 'repository Microkit: %s\n' "$repo_version"
printf 'LionsOS commit: %s\n' "$LIONSOS_COMMIT"
printf 'LionsOS Microkit: %s\n' "$upstream_version"
printf 'LionsOS microkit_sdf_gen: %s\n' "${sdfgen_version:-unknown}"

if [[ "$repo_version" == "$upstream_version" ]]; then
    if [[ "${1:-}" == "--expect-incompatible" ]]; then
        echo "COMPATIBILITY CHANGED: upstream now matches the repository; revisit WP-0" >&2
        exit 1
    fi
    echo "COMPATIBLE VERSION PINS: proceed to a full example build spike"
    exit 0
fi

echo "INCOMPATIBLE: repository Microkit $repo_version; LionsOS requires $upstream_version"
if [[ "${1:-}" == "--expect-incompatible" ]]; then
    exit 0
fi
exit 2
