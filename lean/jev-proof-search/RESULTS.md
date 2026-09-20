# First live run: 2026-09-20

Jev selected a successful tactic on the first attempt for all **10 true goals**.
Lean rejected every attempt on the **2 false controls**. Within a budget of
three attempts per goal, Jev solved more goals and used fewer attempts than the
baselines, but its API overhead made the observed search slower overall.

This supports testing Jev as a tactic selector. It does not establish general
mathematical ability, calibrated proof-success probabilities, or an improvement
from feedback: none of the true goals needed a second attempt.

## Setup

- Lean 4.19.0, `Std` only, inside a rootless Podman container.
- Jev model requested and returned: `jev-1.13.0`.
- Twelve fixed goals, six fixed tactics, maximum three attempts per goal.
- Ten goals are true; two are false and intentionally have no successful proof.
- API receipts, exact statements, proof sources, diagnostics, and axiom audits
  are in [results/live-2026-09-20](results/live-2026-09-20).
- A native smoke run of the candidate matrix was inspected before the live run.
  The problems, tactics, and Jev instructions were unchanged afterward. This
  is a development experiment, not a preregistered or held-out benchmark.

## Comparison

| Policy | True goals solved / 10 | Attempts, including controls | Lean time | API time | Combined time |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fixed order: `c0,c1,c2,c3,c4,c5` | 7 | 30 | 8.81 s | 0 | 8.81 s |
| Uniform random order: exact expectation | 6.1 | 28.8 | 8.29 s | 0 | 8.29 s |
| Best fixed order, chosen retrospectively | 9 | 24 | 7.20 s | 0 | 7.20 s |
| Jev with failure feedback | **10** | **16** | **4.90 s** | 6.81 s | 11.71 s |

Fixed and random baseline results replay the exhaustive Lean matrix. Random is
an expectation over all 720 candidate permutations, not a sampled result.
The retrospective baseline searches those permutations for the most solved
goals, then the fewest attempts; the selected order is
`c1,c2,c5,c0,c3,c4`. It was selected after observing all outcomes and is included
to show that a stronger fixed heuristic narrows Jev's apparent advantage.

The timing columns sum measured attempt durations; they exclude container
startup, controller overhead, and the extra exhaustive-matrix evaluation.
Each Lean check uses a fresh process and is timed once. Baseline times come
from later matrix checks, while Jev times come from its actual search.
These observations do not establish a statistically reliable latency ranking.

Jev used **16 requests, 10,120 input tokens, and 1,056 output tokens**. No dollar
cost is inferred from those counts. All 10 true goals are solvable by at least
one candidate in the exhaustive matrix. Both false controls have zero accepted
candidates among all six tactics.

## What Jev selected

| Goal | Shape | First tactic | Choice confidence | Lean result |
| --- | --- | --- | ---: | --- |
| g01 | Reflexive equality | `rfl` | 1.00 | Verified |
| g02 | List append length | `simp_all` | 0.42 | Verified |
| g03 | Increment preserves a bound | `omega` | 0.88 | Verified |
| g04 | Construct a conjunction | `constructor <;> assumption` | 0.96 | Verified |
| g05 | Supply an existential witness | `exact ⟨w, hw⟩` | 0.99 | Verified |
| g06 | Recursive doubling equals addition | Induction candidate | 0.84 | Verified |
| g07 | Compose arithmetic inequalities | `omega` | 0.90 | Verified |
| g08 | Reverse a list twice | `simp_all` | 0.50 | Verified |
| g09 | Recursive doubling equals multiplication | Induction candidate | 0.85 | Verified |
| g10 | Discharge an implication | `simp_all` | 0.74 | Verified |
| g11 | False: every `n` satisfies `n + 1 = n` | `omega` | 0.51 | Rejected |
| g12 | False: every `n` equals zero | `omega` | 0.28 | Rejected |

The false goals both followed `omega → induction → simp_all`, exhausting the
budget without proof. The controller marked them unresolved, not verified.
Their falsity is known independently from counterexamples (`n = 0` for g11,
`n = 1` for g12); failed search alone does not prove a statement false.

The choices at confidence 0.42 and 0.50 still produced checked proofs. A
confidence threshold would have rejected useful attempts without adding
correctness evidence. Confidence is a search signal; acceptance depends on Lean.

## Verification and reproducibility

All eight acceptance-boundary tests passed natively, during image build, and in
the restricted runtime. They include actual Lean checks of a valid theorem, a
false goal, and a `sorry` placeholder, plus validation of model-selected IDs and
distributions. All ten exported proof files were rechecked inside the container
with networking disabled.

The image was built from the pinned base digest and checksum-verified Lean
archive in [Containerfile](Containerfile). Runtime image ID:

```text
sha256:90faa5523c3ec79ec3007803dc96c039786166ccf3fc3620e0e146efdfa18020
```

The stored [protocol](results/live-2026-09-20/protocol.json) contains hashes of
the exact harness, goals, tactics, and instructions used for the run. Run:

```sh
bash run-container.sh build
bash run-container.sh test
bash run-container.sh check live-2026-09-20
```

To repeat the live experiment, supply `TYPESAFE_API_KEY` through the environment
and use a new result directory name:

```sh
bash run-container.sh live local-repeat
```

## Next autoresearch experiment

Use a larger, held-out set with several plausible but unsuccessful candidates
per goal. Compare a strong fixed heuristic, one-shot Jev ordering, and adaptive
Jev choices after Lean feedback under the same total time and token budgets.
That would test whether failure feedback helps. Only then add a generator that
proposes new lemmas or proof bodies, retaining the original goals and axiom policy.
