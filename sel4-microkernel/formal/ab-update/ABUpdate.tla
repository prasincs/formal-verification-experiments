---- MODULE ABUpdate ----
(***************************************************************************)
(* A/B firmware update crash-safety model (WP-10).                        *)
(*                                                                         *)
(* Verifies the Tier-3 claim: a crash at any point in the update          *)
(* sequence leaves the boot selector pointing at a bootable (old or       *)
(* verified-new) image.  Each action is one durable implementation step;  *)
(* Crash may strike between any two of them, including while the system   *)
(* is idle.  The atomicity assumptions this abstraction imposes on the    *)
(* implementation are listed in README.md.                                 *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    Slots,        \* the two A/B slots
    MaxCrashes,   \* crash budget (keeps the state space finite)
    MaxAttempts,  \* update-attempt budget: retries after crash or revert
    Buggy         \* TRUE seeds the flag-before-verify ordering bug

ASSUME /\ Cardinality(Slots) = 2
       /\ MaxCrashes \in Nat
       /\ MaxAttempts \in Nat \ {0}
       /\ Buggy \in BOOLEAN

VARIABLES
    image,      \* per-slot content: what a boot-time check would find
    active,     \* slot of the last confirmed-good boot
    flag,       \* durable boot selector the bootloader reads
    phase,      \* installer/boot state machine phase
    confirmed,  \* TRUE iff the system settled after its last boot
    crashes,    \* crashes consumed so far
    attempts    \* update attempts consumed so far

vars == <<image, active, flag, phase, confirmed, crashes, attempts>>

ImageStates == {"empty", "old", "partial", "candidate", "signed"}
Phases == {"idle", "writing", "candidate", "verified", "flagged",
           "first_boot", "bricked"}

\* CHOOSE is deterministic here: with exactly two slots there is a
\* unique witness, so this is safe even under symmetry reduction.
Other(slot) == CHOOSE candidate \in Slots : candidate # slot

\* A slot the bootloader would accept: previously confirmed content or a
\* fully verified candidate.
Valid(slot) == image[slot] \in {"old", "signed"}

Init ==
    /\ active \in Slots
    /\ flag = active
    /\ image = [slot \in Slots |-> IF slot = active THEN "old" ELSE "empty"]
    /\ phase = "idle"
    /\ confirmed = TRUE
    /\ crashes = 0
    /\ attempts = 0

\* Begin (or retry) an update: erase/write the inactive slot, never the
\* active one.  Bounded by MaxAttempts so retries after a crash or a
\* revert stay inside a finite state space.
StartWrite ==
    /\ phase = "idle"
    /\ attempts < MaxAttempts
    /\ attempts' = attempts + 1
    /\ LET target == Other(active) IN
       image' = [image EXCEPT ![target] = "partial"]
    /\ phase' = "writing"
    /\ confirmed' = FALSE
    /\ UNCHANGED <<active, flag, crashes>>

FinishWrite ==
    /\ phase = "writing"
    /\ LET target == Other(active) IN
       image' = [image EXCEPT ![target] = "candidate"]
    /\ phase' = "candidate"
    /\ UNCHANGED <<active, flag, confirmed, crashes, attempts>>

VerifyImage ==
    /\ phase = "candidate"
    /\ LET target == Other(active) IN
       image' = [image EXCEPT ![target] = "signed"]
    /\ phase' = "verified"
    /\ UNCHANGED <<active, flag, confirmed, crashes, attempts>>

\* Durable boot-selector update.  The seeded bug widens the guard so the
\* flag can move before verification completes — the classic bricking
\* ordering the safe model must exclude.
FlipFlag ==
    /\ IF Buggy
          THEN phase \in {"writing", "candidate", "verified"}
          ELSE phase = "verified"
    /\ flag' = Other(active)
    /\ phase' = "flagged"
    /\ UNCHANGED <<image, active, confirmed, crashes, attempts>>

FirstBoot ==
    /\ phase = "flagged"
    /\ Valid(flag)
    /\ phase' = "first_boot"
    /\ UNCHANGED <<image, active, flag, confirmed, crashes, attempts>>

Confirm ==
    /\ phase = "first_boot"
    /\ active' = flag
    /\ confirmed' = TRUE
    /\ phase' = "idle"
    /\ UNCHANGED <<image, flag, crashes, attempts>>

\* Watchdog restores the previously confirmed selector.  Modeled as a
\* nondeterministic alternative to Confirm: a failed first boot is
\* abstracted, not forced by a boot-try counter (see README).
Revert ==
    /\ phase = "first_boot"
    /\ flag' = active
    /\ confirmed' = TRUE
    /\ phase' = "idle"
    /\ UNCHANGED <<image, active, crashes, attempts>>

\* Power loss/reset plus the subsequent reboot, collapsed into one
\* action.  Enabled in every phase, idle included (workplan: "a Crash
\* action enabled at every step").  The reboot destination encodes the
\* assumption that the bootloader re-validates the selected slot on
\* every boot: a valid selector boots (back to idle if the system was
\* settled, else into first_boot), an invalid one bricks the device.
Crash ==
    /\ crashes < MaxCrashes
    /\ crashes' = crashes + 1
    /\ phase' = IF ~Valid(flag) THEN "bricked"
                ELSE IF phase = "idle" THEN "idle"
                ELSE "first_boot"
    /\ UNCHANGED <<image, active, flag, confirmed, attempts>>

Next == StartWrite \/ FinishWrite \/ VerifyImage \/ FlipFlag
        \/ FirstBoot \/ Confirm \/ Revert \/ Crash

----------------------------------------------------------------------------
(* Safety *)

TypeOK ==
    /\ image \in [Slots -> ImageStates]
    /\ active \in Slots
    /\ flag \in Slots
    /\ phase \in Phases
    /\ confirmed \in BOOLEAN
    /\ crashes \in 0..MaxCrashes
    /\ attempts \in 0..MaxAttempts

\* The headline claim: the boot selector always points at a bootable
\* image, and the bricked sink is unreachable.
NeverBricked == Valid(flag) /\ phase # "bricked"

\* The fallback is never destroyed: the last confirmed slot stays
\* bootable through every phase of every attempt (writes only ever
\* target the inactive slot).
ActiveSlotAlwaysValid == Valid(active)

\* Stronger than NeverBricked at the moment of the flip: whenever the
\* selector has moved away from the confirmed slot, the slot it selects
\* is fully verified — "old but wrong slot" is not good enough.
FlagFlippedImpliesSigned == (flag # active) => (image[flag] = "signed")

\* confirmed is exactly the settled idle state, and a settled system
\* always has its selector on a valid confirmed slot.
ConfirmedConsistency ==
    /\ confirmed <=> (phase = "idle")
    /\ confirmed => (flag = active)

----------------------------------------------------------------------------
(* Liveness *)

UpdateAwaitingConfirmation == attempts > 0 /\ ~confirmed

\* NOTE: satisfied by Revert as well as Confirm — this proves the
\* machine always settles back into a confirmed boot, NOT that a started
\* update is eventually applied (see README).
EventuallyConfirmed == UpdateAwaitingConfirmation ~> confirmed

\* Stronger settling claim: with finite crash and attempt budgets the
\* system eventually stays confirmed forever (no livelock of endless
\* update/revert churn).
EventuallySettled == <>[]confirmed

----------------------------------------------------------------------------
(* Fairness: the installer, verifier, bootloader and watchdog make      *)
(* progress; StartWrite (starting an update is optional) and Crash (the *)
(* environment) are deliberately unfair.                                *)

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(FinishWrite)
    /\ WF_vars(VerifyImage)
    /\ WF_vars(FlipFlag)
    /\ WF_vars(FirstBoot)
    /\ WF_vars(Confirm)
    /\ WF_vars(Revert)

====
