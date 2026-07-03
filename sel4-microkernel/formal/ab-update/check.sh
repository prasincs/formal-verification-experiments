#!/usr/bin/env bash
set -euo pipefail

TLA_JAR="${TLA_JAR:-/tmp/tla2tools.jar}"
java -XX:+UseParallelGC -cp "$TLA_JAR" tlc2.TLC -config ABUpdate.cfg ABUpdate.tla

set +e
java -XX:+UseParallelGC -cp "$TLA_JAR" tlc2.TLC -config ABUpdateBug.cfg ABUpdate.tla
status=$?
set -e

if [[ $status -eq 0 ]]; then
  echo "counterexample configuration unexpectedly satisfied NeverBricked" >&2
  exit 1
fi

echo "safe model passed and counterexample was detected"
