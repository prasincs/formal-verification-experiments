import Std
set_option autoImplicit false
set_option warningAsError true
set_option maxHeartbeats 200000

def twice : Nat → Nat
  | 0 => 0
  | n + 1 => twice n + 2

theorem trial (P Q : Prop) (hp : P) : Q → P := by
  simp_all

#print axioms trial
