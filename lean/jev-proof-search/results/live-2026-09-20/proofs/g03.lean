import Std
set_option autoImplicit false
set_option warningAsError true
set_option maxHeartbeats 200000

def twice : Nat → Nat
  | 0 => 0
  | n + 1 => twice n + 2

theorem trial (n limit : Nat) (h : n < limit) : n + 1 ≤ limit := by
  omega

#print axioms trial
