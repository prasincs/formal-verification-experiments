"""Check the proof-acceptance boundary with real Lean, not mocked success."""

import copy
import os
import unittest

import experiment as experiment


class ProofBoundaryTests(unittest.TestCase):
    lean = os.environ.get("LEAN_BIN", "lean")

    def test_real_proof_is_accepted(self):
        result = experiment.verify(
            self.lean, {"binders": "(n : Nat)", "target": "n = n"}, "rfl", 10
        )
        self.assertEqual(result["status"], "verified", result["diagnostic"])
        self.assertEqual(result["axioms"], [])

    def test_false_goal_is_rejected(self):
        result = experiment.verify(
            self.lean, {"binders": "", "target": "False"}, "omega", 10
        )
        self.assertEqual(result["status"], "rejected")

    def test_sorry_cannot_pass_as_a_proof(self):
        result = experiment.verify(
            self.lean, {"binders": "", "target": "False"}, "sorry", 10
        )
        self.assertEqual(result["status"], "rejected")

    def test_undeclared_axiom_is_not_allowed(self):
        axioms = experiment.checked_axioms("'trial' depends on axioms: [sorryAx]")
        self.assertFalse(set(axioms) <= experiment.ALLOWED_AXIOMS)


class ModelBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.response = {
            "model": experiment.MODEL,
            "answers": {"next_tactic": {
                "type": "choice", "choice": "c0", "confidence": 0.5,
                "probabilities": {"c0": 0.75, "c1": 0.25},
            }},
        }

    def test_valid_answer(self):
        self.assertEqual(experiment.validate_answer(self.response, ["c0", "c1"])["choice"], "c0")

    def test_model_cannot_introduce_code(self):
        self.response["answers"]["next_tactic"]["choice"] = "sorry"
        with self.assertRaises(ValueError):
            experiment.validate_answer(self.response, ["c0", "c1"])

    def test_tried_candidate_cannot_be_selected_again(self):
        with self.assertRaises(ValueError):
            experiment.validate_answer(self.response, ["c1"])

    def test_nonfinite_confidence_and_bad_distribution_are_rejected(self):
        for field, value in (("confidence", float("nan")),
                             ("probabilities", {"c0": 1.0, "c1": 1.0})):
            response = copy.deepcopy(self.response)
            response["answers"]["next_tactic"][field] = value
            with self.assertRaises(ValueError):
                experiment.validate_answer(response, ["c0", "c1"])


if __name__ == "__main__":
    unittest.main()
