import Std
set_option autoImplicit false
set_option warningAsError true
set_option maxHeartbeats 200000

def twice : Nat → Nat
  | 0 => 0
  | n + 1 => twice n + 2

theorem trial (a b c : Nat) (h1 : a ≤ b) (h2 : b < c) : a < c := by
  omega

#print axioms trial
