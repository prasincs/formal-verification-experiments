# A/B update crash-safety model

This model checks the Tier-3 claim that a crash at any point — mid-update or
idle — leaves the boot flag pointing at a bootable image, and that the system
always settles back into a confirmed boot within finite crash and retry
budgets.

Path note: the workplan says `formal/ab-update/`; per the ownership matrix's
path convention this lives under `sel4-microkernel/formal/ab-update/`.

## Model-to-implementation map

| TLA+ action | Future implementation point |
|---|---|
| `StartWrite` | erase/write inactive slot, never the active slot (retried after crash or revert, up to `MaxAttempts`) |
| `FinishWrite` | finish payload transfer and close the image |
| `VerifyImage` | capsule hash/signature/binding checks complete |
| `FlipFlag` | durable boot-selector update after verification |
| `FirstBoot` | boot candidate under watchdog supervision |
| `Confirm` | persist successful first-boot confirmation |
| `Revert` | watchdog restores the previously confirmed selector |
| `Crash` | power loss/reset at any point, including while idle |

## Atomicity assumptions (implementation obligations)

The model's atomic actions impose these obligations on the installer and
bootloader; if any is violated the model's guarantees do not transfer:

1. **The selector write is atomic and durable** — `FlipFlag` is a single
   action, so the implementation must use a single-sector or TPM-NV write
   with no torn intermediate state.
2. **The bootloader re-validates the selected slot on every boot** —
   `Crash`'s reboot destination is keyed on `Valid(flag)`, which assumes a
   boot-time signature check, not trust in the flag alone.
3. **A failed first boot is abstracted, not forced** — `Revert` is a
   nondeterministic watchdog choice; there is no boot-try counter. The real
   watchdog must guarantee that a candidate that never confirms eventually
   reverts (the model checks both outcomes are safe, not the trigger
   mechanism).

## Properties

Safety (invariants):

- `TypeOK` — domains of all variables (checked first; catches typos).
- `NeverBricked` — the selector always points at an old or verified-signed
  image, and the `bricked` sink is unreachable.
- `ActiveSlotAlwaysValid` — the fallback slot is never destroyed: writes only
  ever target the inactive slot, across all retries.
- `FlagFlippedImpliesSigned` — whenever the selector has moved off the
  confirmed slot, the slot it points at is fully verified.
- `ConfirmedConsistency` — `confirmed` holds exactly in the settled idle
  state, with the selector on the confirmed slot.

Liveness (temporal properties, under the weak fairness in `Spec`):

- `EventuallyConfirmed` — a pending update leads to a confirmed state.
  **Scope caveat:** `Revert` satisfies this as well as `Confirm`, so it
  proves "the machine settles into a confirmed boot", *not* "a started
  update is eventually applied". An implementation that always reverts
  satisfies it.
- `EventuallySettled` (`<>[]confirmed`) — with finite budgets there is no
  endless update/revert churn: every behavior ends settled.

Fairness is deliberately weak (`WF_vars`) on the progress actions only.
`StartWrite` (starting an update is optional) and `Crash` (the environment)
carry no fairness. Strong fairness is not needed: no progress action is
repeatedly enabled-then-disabled without being taken.

## Scope and state space

- Updates are bounded by `MaxAttempts = 2`, so retry-after-crash,
  retry-after-revert, and a second full update cycle (flag churn in both
  directions) are inside the checked space. Crashes are bounded by
  `MaxCrashes = 3`.
- All variables are finite; there are no unbounded counters or sequences,
  so TLC exhausts the state space without state constraints.
- Slots are plain integers `{0, 1}`, not a symmetry set: TLC's symmetry
  reduction is unsound when checking liveness properties, and the state
  space (hundreds of states) doesn't need it. `Other(...)`'s `CHOOSE` has a
  unique witness, so this choice is safe either way.

## Run TLC

```sh
curl --fail -L -o /tmp/tla2tools.jar \
  https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar
TLA_JAR=/tmp/tla2tools.jar bash check.sh
```

CI runs `check.sh` with a pinned, checksummed TLC and requires: the safe
model exits 0 **and** reports exhaustive completion **and** actually checked
the temporal properties; the seeded model fails **with** a `NeverBricked`
violation **and** a counterexample trace. Both logs are uploaded as
artifacts, so the counterexample is preserved as evidence.

## Seeded bug

`ABUpdateBug.cfg` sets `Buggy = TRUE`, allowing `FlipFlag` while the inactive
slot is partial or merely copied but not verified. TLC must report a
`NeverBricked` violation. CI treats a zero exit code for this model as a
failure, so the counterexample proves the invariant is sensitive to the
write/verify/flag ordering rather than vacuously true.

## Non-goals

Tier-2 hot updates require a separate model for TPM NV increment, PCR
extension, staging, slot write, and restart. This A/B model intentionally
does not claim to cover that ordering; it is a blocking prerequisite for
WP-12.
