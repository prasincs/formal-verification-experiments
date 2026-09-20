import Std
set_option autoImplicit false
set_option warningAsError true
set_option maxHeartbeats 200000

def twice : Nat → Nat
  | 0 => 0
  | n + 1 => twice n + 2

theorem trial (n : Nat) : twice n = n + n := by
  induction n <;> simp_all [twice, Nat.add_assoc, Nat.add_comm, Nat.add_left_comm, Nat.mul_succ]

#print axioms trial
