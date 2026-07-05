#!/usr/bin/env bash
set -euo pipefail

TLA_JAR="${TLA_JAR:-/tmp/tla2tools.jar}"

# --- Safe model: must exhaust the state space with no violation -------------
set +e
java -XX:+UseParallelGC -cp "$TLA_JAR" tlc2.TLC -config ABUpdate.cfg ABUpdate.tla \
  2>&1 | tee safe-model.log
safe_status=${PIPESTATUS[0]}
set -e
if [[ $safe_status -ne 0 ]]; then
  echo "safe model failed with status $safe_status" >&2
  exit "$safe_status"
fi
# Exit code 0 is not enough evidence on its own: also require the
# exhaustive-completion banner and proof that liveness was actually
# evaluated, so silently dropping PROPERTIES from the cfg fails the gate.
if ! grep -q "Model checking completed. No error has been found" safe-model.log; then
  echo "safe model exited 0 but did not report exhaustive completion" >&2
  exit 1
fi
if ! grep -q "Checking temporal properties" safe-model.log; then
  echo "safe model never checked the temporal properties" >&2
  exit 1
fi

# --- Seeded bug: must be caught, and caught for the right reason ------------
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

if ! grep -q "The behavior up to this point is" seeded-bug.log; then
  echo "seeded model violated NeverBricked but produced no counterexample trace" >&2
  exit 1
fi

echo "safe model passed and the seeded NeverBricked counterexample was detected"
