---- MODULE ABUpdate ----
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Slots, MaxCrashes, Buggy

ASSUME /\ Cardinality(Slots) = 2
       /\ MaxCrashes \in Nat
       /\ Buggy \in BOOLEAN

VARIABLES image, active, flag, phase, confirmed, crashes, updateDone

vars == <<image, active, flag, phase, confirmed, crashes, updateDone>>

ImageStates == {"empty", "old", "partial", "candidate", "signed"}
Phases == {"idle", "writing", "candidate", "verified", "flagged", "first_boot", "bricked"}

Other(slot) == CHOOSE candidate \in Slots : candidate # slot
Valid(slot) == image[slot] \in {"old", "signed"}

Init ==
    /\ active \in Slots
    /\ flag = active
    /\ image = [slot \in Slots |-> IF slot = active THEN "old" ELSE "empty"]
    /\ phase = "idle"
    /\ confirmed = TRUE
    /\ crashes = 0
    /\ updateDone = FALSE

StartWrite ==
    /\ phase = "idle"
    /\ ~updateDone
    /\ LET target == Other(active) IN
       image' = [image EXCEPT ![target] = "partial"]
    /\ phase' = "writing"
    /\ confirmed' = FALSE
    /\ updateDone' = TRUE
    /\ UNCHANGED <<active, flag, crashes>>

FinishWrite ==
    /\ phase = "writing"
    /\ LET target == Other(active) IN
       image' = [image EXCEPT ![target] = "candidate"]
    /\ phase' = "candidate"
    /\ UNCHANGED <<active, flag, confirmed, crashes, updateDone>>

VerifyImage ==
    /\ phase = "candidate"
    /\ LET target == Other(active) IN
       image' = [image EXCEPT ![target] = "signed"]
    /\ phase' = "verified"
    /\ UNCHANGED <<active, flag, confirmed, crashes, updateDone>>

FlipFlag ==
    /\ IF Buggy
          THEN phase \in {"writing", "candidate", "verified"}
          ELSE phase = "verified"
    /\ flag' = Other(active)
    /\ phase' = "flagged"
    /\ UNCHANGED <<image, active, confirmed, crashes, updateDone>>

FirstBoot ==
    /\ phase = "flagged"
    /\ Valid(flag)
    /\ phase' = "first_boot"
    /\ UNCHANGED <<image, active, flag, confirmed, crashes, updateDone>>

Confirm ==
    /\ phase = "first_boot"
    /\ active' = flag
    /\ confirmed' = TRUE
    /\ phase' = "idle"
    /\ UNCHANGED <<image, flag, crashes, updateDone>>

Revert ==
    /\ phase = "first_boot"
    /\ flag' = active
    /\ confirmed' = TRUE
    /\ phase' = "idle"
    /\ UNCHANGED <<image, active, crashes, updateDone>>

Crash ==
    /\ phase # "idle"
    /\ crashes < MaxCrashes
    /\ crashes' = crashes + 1
    /\ phase' = IF Valid(flag) THEN "first_boot" ELSE "bricked"
    /\ UNCHANGED <<image, active, flag, confirmed, updateDone>>

Next == StartWrite \/ FinishWrite \/ VerifyImage \/ FlipFlag \/ FirstBoot \/ Confirm \/ Revert \/ Crash

TypeOK ==
    /\ image \in [Slots -> ImageStates]
    /\ active \in Slots
    /\ flag \in Slots
    /\ phase \in Phases
    /\ confirmed \in BOOLEAN
    /\ crashes \in 0..MaxCrashes
    /\ updateDone \in BOOLEAN

NeverBricked == Valid(flag)
UpdateAwaitingConfirmation == updateDone /\ ~confirmed
EventuallyConfirmed == UpdateAwaitingConfirmation ~> confirmed

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
