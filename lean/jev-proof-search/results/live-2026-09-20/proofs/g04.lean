import Std
set_option autoImplicit false
set_option warningAsError true
set_option maxHeartbeats 200000

def twice : Nat → Nat
  | 0 => 0
  | n + 1 => twice n + 2

theorem trial (P Q : Prop) (hp : P) (hq : Q) : P ∧ Q := by
  constructor <;> assumption

#print axioms trial
