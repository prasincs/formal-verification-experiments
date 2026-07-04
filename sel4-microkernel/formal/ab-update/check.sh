#!/usr/bin/env bash
set -euo pipefail

TLA_JAR="${TLA_JAR:-/tmp/tla2tools.jar}"

set +e
java -XX:+UseParallelGC -cp "$TLA_JAR" tlc2.TLC -config ABUpdate.cfg ABUpdate.tla \
  2>&1 | tee safe-model.log
safe_status=${PIPESTATUS[0]}
set -e
if [[ $safe_status -ne 0 ]]; then
  echo "safe model failed with status $safe_status" >&2
  exit "$safe_status"
fi

set +e
java -XX:+UseParallelGC -cp "$TLA_JAR" tlc2.TLC -config ABUpdateBug.cfg ABUpdate.tla \
  2>&1 | tee seeded-bug.log
bug_status=${PIPESTATUS[0]}
set -e

if [[ $bug_status -eq 0 ]]; then
  echo "counterexample configuration unexpectedly satisfied NeverBricked" >&2
  exit 1
fi

if ! grep -q "Invariant NeverBricked is violated" seeded-bug.log; then
  echo "seeded model failed, but not because NeverBricked was violated" >&2
  exit 1
fi

echo "safe model passed and the seeded NeverBricked counterexample was detected"
