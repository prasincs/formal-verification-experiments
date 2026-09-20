import Std
set_option autoImplicit false
set_option warningAsError true
set_option maxHeartbeats 200000

def twice : Nat → Nat
  | 0 => 0
  | n + 1 => twice n + 2

theorem trial (xs ys : List Nat) : (xs ++ ys).length = xs.length + ys.length := by
  simp_all

#print axioms trial
