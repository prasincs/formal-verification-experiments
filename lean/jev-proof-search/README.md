# Jev-guided Lean proof search

A small experiment asking whether Jev can choose useful proof attempts. The
model chooses a tactic ID; Lean checks the fixed theorem. This is a bounded
proof-search loop, not a trained theorem prover or an autonomous discovery result.

The [first live run](RESULTS.md) solved all 10 true goals on the first attempt
and accepted neither false control. It reduced proof attempts, but API overhead
outweighed the saved Lean time on these small goals.

## Protocol

- Lean is pinned to **4.19.0**, using `Std` only; no Mathlib dependencies.
- Jev is pinned to **jev-1.13.0**.
- Twelve hand-written goals: ten true statements and two false controls.
- Every goal has the same six literal tactic candidates.
- Each policy may try at most **three distinct candidates** per goal.
- Jev receives the complete statement, local definitions, remaining tactics,
  and diagnostics from its own failed attempts. It gets another choice after a
  failure. Ground-truth annotations and untried outcomes are not sent.
- An exhaustive 12 × 6 Lean matrix is computed **after** the Jev search.
- Baselines replay that matrix: fixed candidate order, and the exact expectation
  over all 720 uniformly random orders without replacement. Random results may
  therefore be fractional. They are expectations, not observed runs.
- Protocol, dataset, tactic, and harness hashes are saved before evaluation.

The corpus is deliberately tiny and includes easy, overlapping tactic cases.
Candidate tactics are hand-written to cover the kinds of goals in the suite.
There is no train/test split or independent assessment of generalization.
Do not interpret these results as a broad math benchmark or calibration study.

## Run

### Container (recommended)

Podman or Docker is sufficient; Lean and Python need not be installed on the
host. The current image targets **Linux x86_64**, pins the Python base image
digest, and verifies the Lean release archive's SHA-256. System package versions
are resolved at build time, so the full build is not bit-for-bit reproducible.

```sh
bash run-container.sh build
bash run-container.sh test
bash run-container.sh offline local-offline
bash run-container.sh check local-offline
```

For a live run with `TYPESAFE_API_KEY` already in the environment:

```sh
bash run-container.sh live local-live
bash run-container.sh check local-live
```

Set `CONTAINER_RUNTIME=docker` to use Docker. Run from any working directory;
receipts are saved under this experiment's `results/` directory. The runtime has
a read-only root filesystem, a temporary writable `/tmp`, and a writable results
mount. Offline runs, tests, and proof rechecks have no network access. Live mode
enables networking and forwards the key by environment-variable name; neither
the build context nor the image contains the key. Rootless Podman uses the
calling user's identity for result-file ownership.

### Native

Install the toolchain named in `lean-toolchain` using elan, or pass an absolute
path to a standalone Lean 4.19.0 binary. Python 3.10+ and its standard library
are sufficient.

```sh
python3 experiment.py --output results/local-offline
```

An offline run checks all candidates and computes baselines. It does **not**
simulate a Jev response or claim any Jev performance.

For a live run, make `TYPESAFE_API_KEY` available in the environment, then:

```sh
python3 experiment.py --live --output results/local-live
```

This sends the toy theorem statements, candidate tactics, and Lean diagnostics
to TypeSafe. At most 36 billable requests are made with the default budget.
The key is never put on the command line or stored in receipts. The harness
does not load `.env` files automatically. Each output directory must be new.
Network/provider errors stop the run, preserving completed goal receipts;
they never produce a fabricated model choice or successful proof.

```sh
LEAN_BIN=/path/to/lean python3 -m unittest -v
```

## Evidence and interpretation

The output directory contains:

- `protocol.json`: frozen configuration and hashes.
- `jev.jsonl`: live requests, structured responses, token usage, and actual
  Lean results for each attempted tactic (live mode only).
- `matrix.jsonl`: every goal/candidate result, axiom audit, and diagnostic.
- `summary.json`: success within budget, attempt counts, and timing.
- `proofs/*.lean`: successful Jev-selected proofs in live mode, or first
  successful matrix proofs in offline mode. These can be rechecked with Lean.

Unsuccessful searches remain unresolved. The two false goals are controls
because their mathematical statements have explicit counterexamples; a failed
tactic search by itself does not establish falsity.

Lean timings use fresh processes, include startup overhead, and are one sample
per attempt. Baseline time is reconstructed from the matrix; Jev time comes
from its live attempts. API time is reported separately. The exhaustive matrix
is evaluation overhead, not part of a deployed search policy. This is an
attempt-efficiency experiment, not a rigorous speed comparison. Token usage is
reported rather than assuming a dollar price.

## Trust boundary

Only predeclared tactic IDs can be selected. No model-written Lean or shell code
is executed. Each attempt compiles the original statement as a theorem, treats
warnings as errors, and audits `#print axioms trial`. Only Lean's standard
`propext`, `Classical.choice`, and `Quot.sound` are allowed; `sorryAx` is rejected.
Confidence never changes proof acceptance. Time and heartbeat limits bound each
attempt, and failed attempts do not modify subsequent theorem statements.

This is an experiment over trusted, hand-written source files, not a sandbox
for arbitrary third-party Lean programs. Its guarantees remain relative to the
Lean toolchain, standard axioms, and fixed formal statements.

## Connection to mathematical autoresearch

This isolates one component of a larger loop:

1. A generator or library supplies candidate lemmas, tactics, or conjectures.
2. A selector orders the experiments.
3. Lean supplies checked outcomes and failure feedback.
4. A controller updates search priorities within a fixed budget.

A next experiment could compare feedback-driven choice to one-shot Jev ranking
on held-out goals, then test candidate generation separately. Preserve the
target statement and permitted axioms; reward checked proofs and useful checked
lemmas, not increased confidence or weakened goals.

## References

- [TypeSafe Choice API](https://docs.typesafe.ai/primitives/choice.md)
- [Jev 1.13 limitations](https://docs.typesafe.ai/model-jaggedness/jev-1.13.md)
- [Lean kernel and theorem proving](https://lean-lang.org/theorem_proving_in_lean4/Introduction/)
- [Lean axioms](https://lean-lang.org/theorem_proving_in_lean4/Axioms-and-Computation/)
- [LeanDojo premise selection and proof search](https://arxiv.org/abs/2306.15626)
