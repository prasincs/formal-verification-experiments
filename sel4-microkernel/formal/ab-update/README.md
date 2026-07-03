# A/B update crash-safety model

This model checks the Tier-3 claim that a crash at any update step leaves the
boot flag pointing at a signed image, and that a started update eventually
returns to a confirmed state once the finite crash budget is exhausted.

## Model-to-implementation map

| TLA+ action | Future implementation point |
|---|---|
| `StartWrite` | erase/write inactive slot, never the active slot |
| `FinishWrite` | finish payload transfer and close the image |
| `VerifyImage` | capsule hash/signature/binding checks complete |
| `FlipFlag` | durable boot-selector update after verification |
| `FirstBoot` | boot candidate under watchdog supervision |
| `Confirm` | persist successful first-boot confirmation |
| `Revert` | watchdog restores the previously confirmed selector |
| `Crash` | power loss/reset between any two durable operations |

`NeverBricked` requires the selected boot slot to contain an old or newly
verified signed image. `EventuallyConfirmed` is a liveness property under weak
fairness, with at most three crashes in the checked configuration.

## Run TLC

```sh
curl -L -o tla2tools.jar \
  https://github.com/tlaplus/tlaplus/releases/latest/download/tla2tools.jar
java -XX:+UseParallelGC -cp tla2tools.jar tlc2.TLC -config ABUpdate.cfg ABUpdate.tla
```

The CI workflow runs that command and requires success.

## Seeded bug

`ABUpdateBug.cfg` sets `Buggy = TRUE`, allowing `FlipFlag` while the inactive
slot is partial or merely copied but not verified. TLC must report a
`NeverBricked` violation. CI treats a zero exit code for this model as a
failure, so the counterexample proves the invariant is sensitive to the
write/verify/flag ordering rather than vacuously true.

Tier-2 hot updates require a separate model for TPM NV increment, PCR
extension, staging, slot write, and restart. This A/B model intentionally does
not claim to cover that ordering.
