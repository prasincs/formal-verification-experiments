# Substrate decision — LionsOS / sDDF (WP-0)

**Decision: PARTIAL.** Adopt sDDF's *patterns* now; keep a concrete path to
lifting specific BSD-2-Clause sDDF components later behind its own
`extern` OS shim; do **not** migrate the substrate or bump Microkit in
Wave 1.

This memo is backed by a reproducible spike, not version-string
inference:
[`scripts/spikes/sddf-compat.sh`](../scripts/spikes/sddf-compat.sh).
Everything below marked *(spike)* is printed by that script; re-run it to
reproduce. Pins evaluated: sDDF `e7788aad` (2026-07, post-0.6.0), our
Microkit `2.1.0` (`build-system/config/versions.mk`), our toolchain
`nightly-2026-07-02` + system `clang 18`.

## The question (WP-0)

Do Phases A+ build on LionsOS/sDDF services, or on this repo's bespoke
ring plumbing? This gates *direction*, not other Wave-1 WPs — protocol
crates, proofs, the checker, inference, TPM, and capsules are
substrate-independent and proceeded regardless.

## What the spike establishes

### 1. Hard version coupling: sDDF HEAD needs Microkit 2.2.0; we pin 2.1.0 *(spike)*

sDDF's own README requires **Microkit SDK 2.2.0**
(`README.md` install section). We pin **2.1.0**. This is not cosmetic:
since release 0.6.0 sDDF generates *all* component configuration through
a metaprogram, **`sdfgen==0.33.0`** (sDDF README), which emits the
Microkit system-description (SDF/`.system`) and each PD's config blob.
`sdfgen`'s emitted SDF schema and libmicrokit expectations track the
2.2.0 `microkit` tool; the 2.1.0 tool consumes a different SDF revision.
So adopting an sDDF example is not "add a driver" — it pulls in the 2.2.0
tool **and** the `sdfgen` metaprogram as a build dependency.

Ground rule 1 forbids bumping Microkit ("nightly drift / SDK churn has
broken CI three times"). That rule alone puts *substrate migration* out
of scope for Wave 1. This is the same conclusion the earlier LionsOS
probe reached (LionsOS pins Microkit 2.2.0 and `microkit_sdf_gen`
0.28.1); the sDDF-direct numbers here (2.2.0 + `sdfgen` 0.33.0) confirm
the coupling is in sDDF itself, not just LionsOS's integration.

### 2. Does sDDF *build* against our toolchain? Partially — and the boundary is measurable *(spike)*

The workplan asks specifically whether sDDF's stack "builds against our
pinned toolchain and Microkit 2.1.0." Two honest halves:

- **Full example build: not attempted here** — the Microkit 2.1.0 SDK
  release asset returns **HTTP 403** through this environment's egress
  proxy, so `make examples/serial` (which requires `$MICROKIT_SDK`)
  cannot run in the sandbox. The spike documents the exact URL and code,
  and will attempt `examples/serial` for `qemu_virt_aarch64` automatically
  if run where `MICROKIT_SDK` points at a 2.1.0 SDK. **This is the one
  claim we could not close; it is a fetch limitation, not a finding.**
- **The C itself is toolchain-compatible in isolation** — sDDF refactored
  its OS coupling behind `<os/sddf.h>`, and ships an `include/extern/`
  "bring-your-own-OS" implementation of that shim. Compiling sDDF's
  device-independent C against the `extern` shim with our `clang 18`
  (`--target=aarch64-none-elf -ffreestanding`, **no Microkit SDK**):
  `util/fsmalloc.c` and `util/bitarray.c` build **clean**; `util/printf.c`
  and `util/cache.c` stop only at `sel4/sel4.h` — i.e. they need the
  seL4/OS integration layer, not a different C dialect.

The takeaway: what blocks a standalone build is the **Microkit-tool /
seL4-SDK version**, not the source language or our compiler era.

### 3. The `extern` shim makes "lift components later" a bounded interface, not a fork *(spike)*

Because sDDF deliberately abstracted its OS dependency, a foreign OS
(our Rust PDs) hosts sDDF code by implementing a **small, enumerated FFI
surface** — the entirety of `include/extern/os/sddf.h`:

```
sddf_get_pd_name        sddf_notify              sddf_deferred_notify
sddf_irq_ack            sddf_deferred_irq_ack    sddf_deferred_notify_curr
sddf_ppcall             sddf_get_mr / sddf_set_mr
sddf_channel (typedef)
```

Every one of these is a thin wrapper over a Microkit/seL4 primitive our
PDs already call (notify, IRQ ack, protected-procedure call, message
registers). Lifting a specific sDDF driver later means: vendor the
driver's C + the sDDF headers, provide those ~11 functions in Rust, and
link — **no upstream fork, no Microkit bump**, provided the driver's C
compiles against the seL4 headers our SDK already ships. That is a
materially cheaper and better-scoped path than a wholesale substrate
migration, and it is why the decision is *partial* rather than *decline*.

### 4. License posture: favorable *(spike + inspection)*

sDDF **code** is `BSD-2-Clause` (per-file SPDX, e.g. `util/*.c`,
`drivers/serial/arm/uart.c`) — permissive, compatible with vendoring
into this repo. Only the **docs** are `CC-BY-SA-4.0` (`LICENSE.md`). No
copyleft obligation attaches to reusing the drivers or utility code.
Maintenance posture: active (UNSW/Trustworthy Systems; the pinned commit
is days old), but the config tooling is self-described as "experimental
and undergoing active development" — another reason not to bind our build
to it yet.

## netdemo on sDDF — what it would look like

Today `netdemo` is a Rust virtio-net driver PD feeding our verified
`rpi4-network-protocol` rings (WP-1 layers smoltcp on top). On sDDF the
shape inverts:

- sDDF's `network/` virtualiser + driver (C) own the NIC and expose
  sDDF's *own* shared-ring ABI (its `net_queue` regions), configured by
  `sdfgen`, not our IC-1 ring header.
- Our client PD would consume sDDF's ring ABI instead of
  `rpi4-network-protocol` — i.e. the verified ring protocol we just
  built (WP-4/WP-11) would be bypassed on the network path, or bridged.
- The Microkit system description would be `sdfgen`-generated rather than
  our hand-written, WP-5-checked `.system` files.

So the migration cost is not just "port a driver": it is **two ABIs and
two config toolchains** (sDDF rings + `sdfgen` vs. our IC-1 rings +
hand-written checked `.system`), plus the C/Rust boundary at every PD
that touches a device. For the *credential-isolation and update* story
this project is actually about, that surface buys little.

## Migration cost for the existing ring protocols

- `rpi4-input-protocol` / `rpi4-network-protocol` are Verus-verified and
  IC-1-shaped (generation header, quiescent reset). sDDF's rings are a
  different, unverified (for our purposes) ABI. Adopting sDDF rings means
  either re-verifying against sDDF's layout or running a bridge PD — both
  cost more than they return in Wave 1.
- The photoframe/decoder and supervisor (WP-3) restart story is built on
  our ring semantics; nothing in sDDF supplies the epoch/quiescent-reset
  discipline those depend on.

## Decision, stated precisely

| Option | Verdict |
|---|---|
| **Adopt** (build Phases A+ on sDDF/LionsOS) | **No** for Wave 1 — requires Microkit 2.2.0 + `sdfgen`, forbidden by ground rule 1. |
| **Partial** | **Yes** — reuse sDDF *patterns* (driver/virtualiser split, queue discipline); keep the `extern`-shim path open to vendoring specific BSD-2-Clause sDDF drivers later without a fork or a Microkit bump. |
| **Decline** (ignore sDDF) | **No** — the license and the `extern` shim make selective future reuse cheap; closing that door would be premature. |

### Revisit triggers

- We decide to move the whole tree to Microkit 2.2.0 for other reasons
  (then re-evaluate full adoption).
- A concrete device need arises that sDDF already drives well (e.g. a
  specific NIC/block controller) — lift that one driver via the `extern`
  shim; the spike already enumerates the FFI to implement.
- The spike's tripwire fires: run
  `scripts/spikes/sddf-compat.sh --expect-incompatible`; a nonzero exit
  means an sDDF example built against our pinned Microkit unaided and
  this memo is stale.

## Non-goals honored

No substrate migrated; no existing product touched; Microkit and
toolchain pins unchanged. The only artifacts added are this memo and the
spike script.

## Reproduce

```bash
sel4-microkernel/scripts/spikes/sddf-compat.sh
# with a Microkit 2.1.0 SDK available, additionally attempts examples/serial:
MICROKIT_SDK=/path/to/microkit-sdk-2.1.0 sel4-microkernel/scripts/spikes/sddf-compat.sh
```

Timebox note (WP-0): the full QEMU-boot half of the spike was blocked by
SDK egress in this environment, documented exactly above rather than
inferred. The build-level and dependency-level findings that drive the
decision were all reproduced by the committed script.
