#!/usr/bin/env python3
"""A fixed-candidate proof-search experiment. Jev chooses; Lean checks.

Only Python's standard library and the pinned Lean toolchain are required.
No model-produced code is executed. See README.md for the trust boundary.
"""

import argparse
import hashlib
import itertools
import json
import math
import os
from pathlib import Path
import re
import statistics
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parent
ENDPOINT = "https://api.typesafe.ai/v1/systemone"
MODEL = "jev-1.13.0"
LEAN_VERSION = "4.19.0"
TACTICS = {
    "c0": "rfl",
    "c1": "simp_all",
    "c2": "omega",
    "c3": "constructor <;> assumption",
    "c4": "exact ⟨w, hw⟩",
    "c5": "induction n <;> simp_all [twice, Nat.add_assoc, Nat.add_comm, Nat.add_left_comm, Nat.mul_succ]",
}
PRELUDE = """import Std
set_option autoImplicit false
set_option warningAsError true
set_option maxHeartbeats 200000

def twice : Nat → Nat
  | 0 => 0
  | n + 1 => twice n + 2

"""
INSTRUCTIONS = (
    "Choose the remaining Lean 4 tactic most likely to completely prove the given "
    "theorem when used as its entire proof body. Read the exact binders and goal. "
    "Each criterion is a literal tactic from a fixed allow-list. "
    "Use any failed attempts as feedback. Do not change the theorem or introduce "
    "assumptions. A proof may be unavailable; in that case choose the most promising "
    "remaining attempt. The controller, not your confidence, decides success by "
    "running Lean."
)
ALLOWED_AXIOMS = {"propext", "Classical.choice", "Quot.sound"}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def theorem_source(problem, tactic):
    return (
        PRELUDE
        + f"theorem trial {problem['binders']} : {problem['target']} := by\n"
        + "\n".join("  " + line for line in tactic.splitlines())
        + "\n\n#print axioms trial\n"
    )


def checked_axioms(output):
    if "'trial' does not depend on any axioms" in output:
        return []
    match = re.search(r"'trial' depends on axioms:\s*\[([^\]]*)\]", output)
    if match:
        return [axiom.strip() for axiom in match.group(1).split(",") if axiom.strip()]
    return None


def verify(lean, problem, tactic, timeout):
    """Compile a closed, fixed theorem and audit its actual axiom dependencies."""
    source = theorem_source(problem, tactic)
    with tempfile.TemporaryDirectory(prefix="jev-lean-check-") as temporary:
        source_path = Path(temporary) / "Trial.lean"
        source_path.write_text(source)
        started = time.perf_counter()
        try:
            result = subprocess.run(
                [lean, str(source_path)], capture_output=True, text=True,
                timeout=timeout, cwd=temporary,
            )
            output = (result.stdout + result.stderr).replace(str(source_path), "Trial.lean")
            axioms = checked_axioms(output)
            accepted = (
                result.returncode == 0 and axioms is not None
                and set(axioms) <= ALLOWED_AXIOMS
            )
            status = "verified" if accepted else "rejected"
        except subprocess.TimeoutExpired:
            output, axioms, status = "Lean attempt timed out.", None, "timeout"
        return {
            "status": status,
            "seconds": time.perf_counter() - started,
            "axioms": axioms,
            "diagnostic": output[:4000],
            "source_sha256": hashlib.sha256(source.encode()).hexdigest(),
        }


def validate_answer(response, remaining):
    if response.get("model") != MODEL:
        raise ValueError("Unexpected resolved model version")
    answers = response.get("answers", {})
    if set(answers) != {"next_tactic"}:
        raise ValueError("Unexpected question IDs")
    answer = answers["next_tactic"]
    probabilities = answer.get("probabilities", {})
    if answer.get("type") != "choice" or answer.get("choice") not in remaining:
        raise ValueError("Model selected an unavailable tactic")
    if set(probabilities) != set(remaining):
        raise ValueError("Probability keys differ from the remaining tactics")
    values = list(probabilities.values()) + [answer.get("confidence")]
    if any(type(value) not in (int, float) or not math.isfinite(value)
           or not 0 <= value <= 1 for value in values):
        raise ValueError("Invalid probability or confidence")
    if not math.isclose(sum(probabilities.values()), 1, abs_tol=0.02):
        raise ValueError("Probabilities do not sum to one within rounding tolerance")
    if probabilities[answer["choice"]] < max(probabilities.values()):
        raise ValueError("Selected tactic does not have maximum probability")
    return answer


def choose(problem, remaining, history, key):
    # Ground-truth annotations and untried outcomes never enter the request.
    request_body = {
        "model": MODEL,
        "state": {
            "lean_version": LEAN_VERSION,
            "context": PRELUDE,
            "theorem": f"theorem trial {problem['binders']} : {problem['target']}",
            "failed_attempts": history,
        },
        "questions": {"next_tactic": {
            "type": "choice", "instructions": INSTRUCTIONS,
            "criteria": {candidate: TACTICS[candidate] for candidate in remaining},
        }},
    }
    request = urllib.request.Request(
        ENDPOINT, data=json.dumps(request_body).encode(), method="POST",
        headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"},
    )
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(request, timeout=45) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as error:
        # Do not print request headers, credentials, or arbitrary provider bodies.
        raise RuntimeError(f"TypeSafe returned HTTP {error.code}") from None
    except urllib.error.URLError:
        raise RuntimeError("TypeSafe network request failed") from None
    elapsed = time.perf_counter() - started
    answer = validate_answer(payload, remaining)
    # Retain only documented fields; never store headers or network metadata.
    receipt = {
        "request": request_body,
        "response": {"model": payload["model"], "answers": payload["answers"],
                     "usage": payload.get("usage", {})},
        "api_seconds": elapsed,
    }
    return answer["choice"], receipt


def run_jev(lean, problem, key, budget, timeout):
    remaining, history, steps = list(TACTICS), [], []
    for _ in range(budget):
        candidate, receipt = choose(problem, remaining, history, key)
        result = verify(lean, problem, TACTICS[candidate], timeout)
        steps.append({"candidate": candidate, "lean": result, **receipt})
        remaining.remove(candidate)
        if result["status"] == "verified":
            break
        history.append({"tactic": TACTICS[candidate], "status": result["status"],
                        "lean_diagnostic": result["diagnostic"]})
    return steps


def simulate(order, results, budget):
    seconds = 0
    for count, candidate in enumerate(order[:budget], start=1):
        seconds += results[candidate]["seconds"]
        if results[candidate]["status"] == "verified":
            return {"solved": 1, "attempts": count, "lean_seconds": seconds}
    return {"solved": 0, "attempts": budget, "lean_seconds": seconds}


def summarize(problems, matrix, live, budget):
    policies = {"fixed": [], "random_exact_expectation": []}
    if live:
        policies["jev_feedback"] = []
    for problem in problems:
        identifier = problem["id"]
        outcomes = matrix[identifier]
        policies["fixed"].append(simulate(list(TACTICS), outcomes, budget))
        random_runs = [simulate(order, outcomes, budget)
                       for order in itertools.permutations(TACTICS)]
        policies["random_exact_expectation"].append({
            key: statistics.mean(run[key] for run in random_runs)
            for key in ("solved", "attempts", "lean_seconds")
        })
        if live:
            steps = live[identifier]
            policies["jev_feedback"].append({
                "solved": int(any(step["lean"]["status"] == "verified" for step in steps)),
                "attempts": len(steps),
                "lean_seconds": sum(step["lean"]["seconds"] for step in steps),
                "api_seconds": sum(step["api_seconds"] for step in steps),
            })
    aggregate = {}
    valid_ids = [index for index, problem in enumerate(problems) if problem["expected_valid"]]
    for policy, rows in policies.items():
        aggregate[policy] = {
            "solved_valid_goals": sum(rows[index]["solved"] for index in valid_ids),
            "total_attempts_including_controls": sum(row["attempts"] for row in rows),
            "lean_seconds": sum(row["lean_seconds"] for row in rows),
            "api_seconds": sum(row.get("api_seconds", 0) for row in rows),
        }
    aggregate["valid_goals"] = len(valid_ids)
    aggregate["negative_controls"] = len(problems) - len(valid_ids)
    aggregate["matrix_valid_goals_solvable_by_candidates"] = sum(
        any(result["status"] == "verified" for result in matrix[problems[index]["id"]].values())
        for index in valid_ids
    )
    if live:
        aggregate["usage"] = {
            field: sum(step["response"]["usage"].get(field, 0)
                       for steps in live.values() for step in steps)
            for field in ("input_tokens", "output_tokens")
        }
    return aggregate


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lean", default="lean")
    parser.add_argument("--live", action="store_true", help="Make billable TypeSafe calls")
    parser.add_argument("--budget", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=10)
    parser.add_argument("--output", type=Path, required=True, help="New directory for results")
    args = parser.parse_args()
    if not 1 <= args.budget <= len(TACTICS) or args.timeout <= 0:
        parser.error("Budget must be 1..6 and timeout must be positive")
    key = os.environ.get("TYPESAFE_API_KEY")
    if args.live and not key:
        parser.error("Set TYPESAFE_API_KEY for a live run (never pass it on the command line)")
    version = subprocess.check_output([args.lean, "--version"], text=True).strip()
    if not version.startswith(f"Lean (version {LEAN_VERSION},"):
        parser.error(f"This experiment requires Lean {LEAN_VERSION}")
    problems = json.loads((ROOT / "problems.json").read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    protocol = {
        "mode": "live" if args.live else "offline_baselines_only",
        "lean_version": version, "model": MODEL if args.live else None,
        "budget": args.budget, "timeout_seconds": args.timeout,
        "problems_sha256": digest(problems), "tactics_sha256": digest(TACTICS),
        "instructions_sha256": digest(INSTRUCTIONS),
        "harness_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "instructions": INSTRUCTIONS, "tactics": TACTICS,
        "note": "Toy suite; fixed order and exact random expectation use the exhaustive Lean matrix. Timing is one sample per attempt, not a speed benchmark.",
    }
    (args.output / "protocol.json").write_text(json.dumps(protocol, indent=2) + "\n")
    live, matrix = {}, {}
    # Run the adaptive search before computing the exhaustive baseline matrix.
    if args.live:
        with (args.output / "jev.jsonl").open("w") as stream:
            for problem in problems:
                live[problem["id"]] = run_jev(args.lean, problem, key, args.budget, args.timeout)
                stream.write(json.dumps({"goal": problem["id"], "steps": live[problem["id"]]}) + "\n")
                stream.flush()
                print(f"{problem['id']}: Jev search finished", flush=True)
    with (args.output / "matrix.jsonl").open("w") as stream:
        for problem in problems:
            results = {candidate: verify(args.lean, problem, tactic, args.timeout)
                       for candidate, tactic in TACTICS.items()}
            matrix[problem["id"]] = results
            stream.write(json.dumps({"goal": problem["id"], "results": results}) + "\n")
            stream.flush()
            successes = [candidate for candidate, result in results.items() if result["status"] == "verified"]
            print(f"{problem['id']}: Lean accepts {successes}", flush=True)
            if not problem["expected_valid"] and successes:
                raise RuntimeError("Negative control was accepted; aborting the experiment")
    proofs = args.output / "proofs"
    proofs.mkdir()
    for problem in problems:
        if args.live:
            winners = [step["candidate"] for step in live[problem["id"]]
                       if step["lean"]["status"] == "verified"]
        else:
            winners = [candidate for candidate, result in matrix[problem["id"]].items()
                       if result["status"] == "verified"]
        if winners:
            (proofs / f"{problem['id']}.lean").write_text(theorem_source(problem, TACTICS[winners[0]]))
    summary = summarize(problems, matrix, live, args.budget)
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
