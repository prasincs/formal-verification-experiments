#!/usr/bin/env bash
set -euo pipefail

experiment_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
runtime="${CONTAINER_RUNTIME:-podman}"
image="${IMAGE:-localhost/jev-lean-proof-search:4.19.0}"
mode="${1:-help}"

usage() {
    printf '%s\n' \
        'Usage: bash run-container.sh build|test|offline|live|check [result-name]' \
        '  build             Build the pinned x86_64 Lean image.' \
        '  test              Run the proof-boundary tests without network access.' \
        '  offline NAME      Check all candidates and record baselines.' \
        '  live NAME         Run Jev feedback search, then compute baselines.' \
        '  check NAME        Recheck saved proof files without network access.' \
        'Set TYPESAFE_API_KEY in the environment for live mode.' \
        'Optional: CONTAINER_RUNTIME=docker, IMAGE=..., RESULTS_DIR=...'
}

case "$mode" in
    build)
        exec "$runtime" build -f "$experiment_dir/Containerfile" -t "$image" "$experiment_dir"
        ;;
    test|offline|live|check) ;;
    help|--help|-h) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
esac

run_args=(run --rm --read-only --cap-drop=ALL --security-opt=no-new-privileges
          --pids-limit=256 --memory=2g
          --tmpfs /tmp:rw,nosuid,nodev,size=256m
          --user "$(id -u):$(id -g)")
if [[ "${runtime##*/}" == podman ]]; then
    run_args+=(--userns=keep-id)
fi

if [[ "$mode" == test ]]; then
    exec "$runtime" "${run_args[@]}" --network=none --entrypoint python3 "$image" -m unittest -v
fi

result_name="${2:?Provide a new result name (or an existing one for check)}"
if [[ ! "$result_name" =~ ^[a-zA-Z0-9][a-zA-Z0-9_-]*$ ]]; then
    printf '%s\n' 'Result names must contain only letters, numbers, underscores, or hyphens.' >&2
    exit 2
fi
results_dir="${RESULTS_DIR:-$experiment_dir/results}"
if [[ "$mode" == live && -z "${TYPESAFE_API_KEY:-}" ]]; then
    printf '%s\n' 'Set TYPESAFE_API_KEY in the environment before a live run.' >&2
    exit 2
fi
if [[ "$mode" == check ]]; then
    test -d "$results_dir/$result_name/proofs"
    run_args+=(--network=none -v "$results_dir/$result_name/proofs:/proofs:ro")
    exec "$runtime" "${run_args[@]}" --entrypoint /bin/sh "$image" \
        -c 'set -eu; for proof in /proofs/*.lean; do test -f "$proof"; lean "$proof"; done'
fi

mkdir -p -- "$results_dir"
results_dir="$(cd -- "$results_dir" && pwd)"
run_args+=(-v "$results_dir:/results:rw")
experiment_args=(--output "/results/$result_name")
if [[ "$mode" == live ]]; then
    # Passing just the variable name keeps its value out of command arguments.
    run_args+=(--env TYPESAFE_API_KEY)
    experiment_args+=(--live)
else
    run_args+=(--network=none)
fi
exec "$runtime" "${run_args[@]}" "$image" "${experiment_args[@]}"
