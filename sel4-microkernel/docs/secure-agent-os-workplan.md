# High-Assurance Agent Appliance — Execution Spec for Parallel Agents

Companion to [secure-agent-os.md](secure-agent-os.md) (the design RFC;
read it first). This document decomposes the design into **work
packages (WPs)** that independent agents can execute concurrently. It
fixes the interface contracts, file ownership, and acceptance criteria
up front so that parallel work composes without coordination.

Execution model: one agent per WP, each on its own branch
(`agent/wp<N>-<slug>`), each landing as its own PR to `main`. Wave 1
WPs are **parallelizable with controlled merge sequencing** — no WP
blocks another's *start*, but there are two acknowledged couplings:
WP-4 merges before WP-11 (both touch `rpi4-network-protocol`,
additively), and WP-3 consumes IC-1 by crate version bump (it ships in
legacy generation-0 mode if WP-4 is late). Wave 2 WPs consume Wave 1
artifacts through the contracts in this file — not through reading
Wave 1 branches.

---

## Ground rules (every agent, every WP)

1. **Toolchain is law.** Rust nightly is pinned by
   `sel4-microkernel/rust-toolchain.toml`; Microkit SDK 2.1.0 and all
   artifact versions by `build-system/config/versions.mk`. Do NOT bump
   either — nightly drift has broken CI three times (see
   `docs/networking-roadmap.md` Phase 8). Use plain `cargo` so the toml
   drives selection.
2. **`no_std` everywhere** in PD code. Host-side tests and tools may use
   std (pattern: `rpi4-photoframe-tests`).
3. **CI green is the definition of alive.** Every WP that produces
   runnable code must extend `.github/workflows` with a QEMU boot test
   modeled on the existing `qemu-netdemo` job (boot, parse serial
   output, assert markers). A WP is not done until its CI job passes.
4. **Verus pattern:** use the `verus!` macro so code builds as plain
   Rust under `cargo build` and verifies under the Verus harness (see
   `verus/README.md` and `rpi4-input-protocol/src/lib.rs` for the house
   style). Proofs live with the code, not in a separate crate.
5. **Protocol crates are append-only.** Never change existing struct
   layouts, constants, or function signatures in
   `rpi4-input-protocol` / `rpi4-network-protocol` / `rpi4-photo-protocol`
   — add alongside.
6. **Stay in your file-ownership lane** (matrix below). If you must
   touch a shared file (`build-system/Makefile`, CI workflow), keep the
   diff additive (new product entry, new job) — never reorganize.
7. **QEMU first, hardware never.** No WP in this spec requires or may
   claim validation on real RPi4 hardware. Document hardware
   assumptions as TODOs in the style of `docs/networking-roadmap.md`
   Phase 5.
8. **Update the paperwork.** Each WP checks off / adds items in
   `docs/networking-roadmap.md` where relevant and adds a row to the
   status table in this file's tracking section via its PR.

---

## Interface contracts

These are fixed. An agent who believes a contract is wrong stops and
reports rather than unilaterally changing it, because another agent is
building against it.

### IC-1: Ring generation header + quiescent reset (WP-3 ⇄ WP-4 ⇄ everything using rings)

The existing ring header (`docs/device-isolation-architecture.md`) is:

```
0x000  u32  write_idx    (atomic, producer-owned)
0x004  u32  read_idx     (atomic, consumer-owned)
0x008  u32  capacity
0x00C  u32  padding      ← becomes `generation`
0x010  ...  entries[]
```

- Offset `0x00C` becomes `generation: u32` (atomic). Value `0` means
  "legacy/no supervisor" — existing images remain layout-compatible.
- **Quiescent reset (normative — review round 2):** a seqlock-style
  double-read was specified here previously and is **rejected**:
  endpoints are *writers* into the state being reset, so the index
  publication necessarily follows the second generation read and the
  reset can land in between — no double-read couples validation to
  publication atomically. Correctness comes from removing concurrency,
  not detecting it. Reset sequence, executed only by the lifecycle PD:

  1. `microkit_pd_stop(producer)`;
  2. `microkit_pd_stop(consumer)` (skip whichever endpoint is the
     lifecycle PD itself);
  3. publish `generation = odd` (release);
  4. zero `write_idx`/`read_idx` and any endpoint-visible ring state;
  5. publish `generation = next even` (release);
  6. restart both endpoints.

- **Endpoint obligations:** on (re)start, acquire-read `generation`;
  if odd, park and raise a fault — that is a lifecycle bug, never a
  retry; record the value and re-derive local cursors from the shared
  indices *before any publication*. During normal operation endpoints
  do **not** need generation checks for correctness (they are stopped
  during resets); a debug-build assertion that the generation is
  unchanged is welcome defense-in-depth, but nothing may rely on it.
- **Topology precondition (checked by WP-5):** both endpoints of every
  restartable ring are children of — or otherwise stoppable by — the
  same lifecycle PD. Rings that cannot satisfy this must not be marked
  restartable; a future acknowledge-quiesce-release handshake protocol
  is out of scope until a concrete ring needs it.
- New API surface (added, not replacing): `generation()` and
  `resync() -> Result<Generation, OddGeneration>` for the (re)start
  path.
- Verus obligation (WP-4): with endpoint-quiescence as an explicit
  precondition of reset, prove reset re-establishes the ring
  invariants and that a restarted endpoint's first publication is
  preceded by `resync`; the lifecycle-side state machine (reset
  unreachable while either endpoint is running) is proven in the
  supervisor work, not assumed.

### IC-2: Update capsule format (WP-8 ⇄ WP-3 ⇄ WP-6)

Little-endian, fixed-offset header (the fixed layout *is* the canonical
serialization), no variable-length fields before the payload. Per
review, the signature must **bind** platform, slot/PD type, load
address, entry point, ABI version, dependencies, rollback epoch, and
expiry — otherwise a validly signed artifact can be replayed into an
unintended slot or under an incompatible interface:

```
0x00  [u8;4]  magic = "SAOC"
0x04  u32     format_version = 2
0x08  u8      payload_type    (1 = pd-code, 2 = model-weights, 3 = config, 4 = wasm-tool)
0x09  u8      target_slot     (slot PD id; 0 for whole-image)
0x0A  u16     target_platform (1 = qemu-aarch64, 2 = rpi4, 3 = qemu-riscv64, ...)
0x0C  u32     abi_version     (slot protocol/ABI the payload expects)
0x10  u64     monotonic_version (rollback epoch)
0x18  u64     payload_len
0x20  u64     load_vaddr      (slot-region base the blob is linked for; 0 = PIC)
0x28  u64     entry_offset    (entry point relative to load_vaddr)
0x30  u64     not_after       (unix seconds; MUST be 0 until a trusted
                               time source is specified — the device
                               has monotonic counters, not wall time;
                               verifiers MUST reject nonzero values
                               they cannot check)
0x38  u32     signer_key_id   (which pinned public key signed this)
0x3C  u32     key_epoch       (key-rotation epoch; capsules signed
                               under a revoked epoch are rejected)
0x40  [u8;32] payload_sha256
0x60  [u8;32] deps_sha256     (digest of dependency/config manifest; zero = none)
0x80  [u8;64] ed25519 signature over bytes [0x00..0x80) ++ payload
0xC0  ...     payload
```

Additional normative semantics:

- **Rollback state is scoped per `(target_slot, payload_type)`** — one
  TPM NV counter per scope, not a single ambiguous global version (a
  model update must not burn the code-slot rollback counter).
- All reserved/zero fields MUST be validated as zero (unknown-field
  smuggling).
- PIC entry: when `load_vaddr == 0`, the entry address is
  `actual slot region base + entry_offset`; when nonzero, the
  installer MUST check `load_vaddr` equals the slot's declared base.

Verification order (normative): parse header (totality — reject, never
trust) → check `payload_len` bounds against buffer → check
`target_platform`, `target_slot`+`payload_type`, `abi_version`,
`signer_key_id`+`key_epoch` against the running system → check
`not_after` (see above) → hash payload → compare `payload_sha256`
(constant-time) → verify signature → check `monotonic_version` against
the scoped NV counter. Only then is the payload **eligible for
installation** (not "readable" — static mappings can't retroactively
hide a shared buffer), and the verifier's sole output is a **one-shot
install authorization**
`{auth_id (fresh nonce), payload_sha256, target_slot, slot_generation,
monotonic_version}`, delivered over the verifier→installer private
channel (channel identity is the authenticity mechanism). The
installer tracks consumed `auth_id`s and rejects reuse; it re-hashes
its own private staging copy against `payload_sha256` before any write
(see design doc Tier-2 for the full handoff — the digest is the
authority, not the buffer).

### IC-3: TPM transport trait (WP-7 ⇄ future keystore)

```rust
pub trait TpmTransport {
    type Error;
    /// Submit a TPM 2.0 command, receive the response.
    fn exchange(&mut self, cmd: &[u8], resp: &mut [u8]) -> Result<usize, Self::Error>;
}
```

`rpi4-tpm-pd`'s existing IPC command surface (`Init`, `PcrExtend`,
`PcrRead`, `GetRandom`, …) is frozen; the trait sits *below* it. The
SLB9670/SPI code becomes the first impl; a TIS/CRB MMIO impl is a
future second. Scope note (review): this trait is for things that
speak TPM 2.0 command streams — crypto engines like NXP CAAM do
**not** qualify; a non-TPM backend would enter via a future
higher-level `AttestationBackend` (measure/seal/counter/quote), not by
stretching this trait.

### IC-4: Supervisor demo system topology (WP-3)

Product `supdemo`, `supdemo.system`: PD `supervisor` (priority 200,
parent) with child PD `worker` (priority 100, `id="1"`). One shared
region `work_ring` (4KB, IC-1 layout) mapped into both; one channel
supervisor(id 1)↔worker(id 1). Worker exposes a "crash on demand"
input: a specific ring entry value causes a deliberate fault.

### IC-5: File ownership matrix

| WP | Owns (create/modify) | Read-only everywhere else |
|---|---|---|
| WP-0 | `docs/substrate-decision.md` (new), spike in scratch branch only | yes |
| WP-1 | `rpi4-network/src/stack/`, `rpi4-network/src/time.rs`, `build-system/config/features/networking.mk` (additive) | yes |
| WP-3 | `rpi4-supervisor/` (new), `supdemo` product files, `rpi4-graphics/supdemo.system` or sibling | yes |
| WP-4 | `rpi4-input-protocol/` (additive), `rpi4-network-protocol/` (additive) | yes |
| WP-5 | `tools/system-check/` (new), one CI job | yes |
| WP-6 | `rpi4-llm/` (new), `rpi4-llm-loader/` (new), `llmdemo` product files | yes |
| WP-7 | `rpi4-tpm-boot/src/` (refactor within), `rpi4-tpm-pd/src/` | yes |
| WP-8 | `update-capsule/` (new crate) | yes |
| WP-10 | `formal/ab-update/` (new) | yes |
| WP-11 | `rpi4-network-protocol/` (proof additions only) | yes |

WP-4 and WP-11 both touch `rpi4-network-protocol`: WP-4 adds the
generation field/API, WP-11 adds proofs to *existing* code. Both are
additive; merge order WP-4 → WP-11 preferred, and WP-11 must not
assume the generation field exists.

---

## Wave 1 — parallelizable with controlled merge sequencing

### WP-0: Substrate evaluation (LionsOS / sDDF) — timeboxed

**Goal:** decide whether Phases A+ build on LionsOS/sDDF services or
this repo's bespoke plumbing (design doc, Related work). This gates
*direction*, not other Wave-1 WPs — they proceed regardless, since
protocol crates, proofs, checker, inference, TPM, and capsules are
substrate-independent.
**Deliverables:** a decision memo (`docs/substrate-decision.md`)
covering: does sDDF's network/timer/serial stack build against our
pinned toolchain and Microkit 2.1.0; what would netdemo look like on
it; license/maintenance posture; migration cost for the existing ring
protocols; plus a build spike (LionsOS example booting in QEMU from
our build system, or a documented failure).
**Acceptance:** memo answers adopt/partial/decline with evidence;
spike outcome reproducible via a make target or documented exactly.
**Non-goals:** actually migrating anything; touching existing products.
**Timebox:** if the spike exceeds ~2 days of effort, write up findings
and stop — that itself is the answer.

### WP-1: IP stack (smoltcp) + time source

**Goal:** `netdemo` gets DHCP + ICMP echo in QEMU CI
(`docs/networking-roadmap.md` Phase 4).
**Read first:** `rpi4-network/src/` (driver trait, virtio driver),
`rpi4-network-protocol/src/lib.rs`, roadmap Phase 4.
**Deliverables:**
- `time.rs`: monotonic time from `CNTVCT_EL0`/`CNTFRQ_EL0` exposing
  `smoltcp::time::Instant`.
- `stack/`: `smoltcp::phy::Device` impl over the existing
  `NetworkDriver` trait; DHCP client; ICMP responder/prober.
- Retire or implement the empty `net-stack-lwip`/`net-stack-picotcp`
  feature declarations (roadmap says: retire in favor of smoltcp).
**Acceptance (CI):** extended `qemu-netdemo` (or new `qemu-ipdemo`)
job: guest acquires a DHCP lease from slirp, pings `10.0.2.2`, serial
output contains `DHCP OK <ip>` and `PING OK` markers.
**Non-goals:** TCP sockets exposed to clients, TLS, GENET/hardware
paths, multi-client.

### WP-3: Supervisor PD + child restart (first hierarchical-PD use)

**Goal:** prove the PD lifecycle mechanism end-to-end in QEMU.
**Read first:** design doc "Tier 1: restart"; Microkit manual sections
on child PDs, `fault` entry point, `microkit_pd_stop/restart`;
`rpi4-input-pd/src/main.rs` for the PD runtime pattern.
**Deliverables:**
- `rpi4-supervisor/`: supervisor PD implementing IC-4 — fault handler
  logs fault info, runs the IC-1 quiescent reset (the worker is
  stopped by the fault, and the supervisor is itself the other ring
  endpoint, so both ends are quiesced by construction), restarts
  the worker at its entry point; plus a heartbeat watchdog (worker
  writes a counter entry every N iterations; supervisor restarts on
  stall).
- `worker` PD: increments a boot-generation counter in its first ring
  entry after every (re)start; crashes on the designated poison entry.
- Product `supdemo` wired into the build system (copy `tvdemo.mk`
  shape), QEMU target.
**Acceptance (CI):** `qemu-supdemo` job: boot → supervisor injects the
poison entry → worker faults → supervisor restarts it → serial shows
`BOOT GEN 1`, `FAULT CAUGHT`, `BOOT GEN 2`, and a post-restart
heartbeat. Kill-via-watchdog path exercised once as well.
**Non-goals:** code reloading (Tier 2), signature checking, more than
one child, Verus proofs of the supervisor (that's a follow-on once WP-8
lands).
**Structure note (review-driven):** the demo runs as one supervisor
binary, but lay the crate out as distinct modules mirroring the design
doc's decomposition — `lifecycle` (faults/stop/restart/generation resets),
`verifier` (stub here), `installer` (stub here) — so Wave 2 can split
them into separate PDs without a rewrite. `lifecycle` must never gain
an rw mapping to any executable region.

### WP-4: Epoch ring protocol + Verus proofs

**Goal:** restart-safe rings per IC-1, proven.
**Read first:** `rpi4-input-protocol/src/lib.rs` (the house proof
style), design doc "Tier 1".
**Deliverables:** generation field + quiescent-reset API in
`rpi4-input-protocol` (additive; offset `0x00C` per IC-1 — note both
prior designs are explicitly rejected: the naive "zero indices, bump
epoch" races, and the seqlock double-read races too, because endpoints
publish *after* the second read), mirrored in `rpi4-network-protocol`.
API: `generation()`, `resync()`, and a reset entry point whose
contract (documented + Verus precondition) is "callable only with both
endpoints stopped." Verus invariants: index bounds preserved
(existing), reset re-establishes ring validity, a (re)started
endpoint's first publication is preceded by `resync`, `resync` on an
odd generation returns the fatal error and permits no subsequent
operation. Host unit tests: restart of producer, restart of consumer,
restart of both, resync-before-publish enforcement, odd-generation
fatality, and generation wraparound behavior (document the chosen wrap
policy). Include a test documenting the *rejected* concurrent-reset
interleaving (debug assertion fires / API cannot express it) so the
race that killed two spec revisions stays dead.
**Acceptance:** `cargo build` clean (macro strips), Verus verification
passes via the `verus/` harness, all host tests above pass.
**Non-goals:** changing existing entry formats; touching PD binaries
(WP-3 consumes this by crate bump, and ships even if WP-4 is late —
generation 0 = legacy mode).

### WP-5: `.system` topology checker

**Goal:** machine-check the capability-topology claims (design doc
§"Check the `.system` files").
**Read first:** all `*.system` files (`rpi4-graphics/*.system`,
`rpi4-photoframe/photoframe.system`, `rpi4-network/netdemo.system`,
`microkit-hello/hello.system`), the isolation tables in
`docs/device-isolation-architecture.md`.
**Deliverables:** `tools/system-check/` (host Rust, std allowed):
parses a `.system` file into a full authority graph — **not just
memory mappings** (review finding: mappings alone cannot establish the
design doc's non-bypassability claim). The graph must cover:

1. memory maps: PD → {region, perms, kind (device phys_addr vs RAM)},
2. channel endpoints and their PD pairs/ids,
3. protected-procedure direction (`pp="true"` — who may call whom),
4. IRQ ownership,
5. parent/child (nested PD) relationships.

(Scope note: in Microkit the `.system` file fully determines the
generated CSpaces, so checking it *is* checking the capability
distribution — no separate CSpace input exists at this layer.)

Sidecar `<name>.system.props.toml` property language:
```toml
[[shared_only]]           # exactly this set of regions shared between these PDs
pds = ["input", "graphics"]
regions = ["input_ring"]

[[exclusive]]             # region mapped into exactly one PD
region = "uart_regs"
pd = "input"

[[no_device_mmio]]        # PD maps no region with phys_addr, owns no IRQ
pd = "worker"

[[only_channels]]         # PD's complete channel set — nothing else
pd = "agent_core"
peers = ["policy"]

[[no_pp_to]]              # PD may not call this PD via protected procedure
pd = "worker"
target = "keystore"

[[dma_capable]]           # PD owns a DMA-capable device: distinguished
pd = "network"            # trust class, must be explicitly declared

[[restartable_ring]]      # IC-1 quiescent-reset precondition: both
region = "work_ring"      # endpoints are children of (or are) the
lifecycle_pd = "supervisor"  # same lifecycle PD
endpoints = ["supervisor", "worker"]
```
Write sidecars for every existing `.system` file, encoding the claims
the docs already make in prose (including declaring `network` as
`dma_capable` — the checker fails on undeclared DMA-device ownership).
**Acceptance (CI):** new job runs the checker over every
`.system`+sidecar pair; deliberately-broken fixtures prove the checker
fails when (a) a mapping is widened, (b) a channel is added to a PD
with `only_channels`, (c) a device/IRQ appears on a `no_device_mmio`
PD, (d) a `restartable_ring` endpoint is not a child of (or identical
to) the declared lifecycle PD. Checker itself has unit tests.
**Non-goals:** notification *direction* enforcement (Microkit channels
are bidirectional at the kernel level; direction is a protocol
convention — document this honestly in the checker README), scheduling
policy, Verus-verifying the checker (nice-to-have, not required).

### WP-6: Verified-substrate local inference (Route B core)

**Goal:** the novel artifact — a no_std inference PD with a verified
loader, running a tiny model in QEMU CI. Design doc §"Local inference",
Route B.
**Read first:** that section; `docs/decoder-allocation-security.md`
(the allocation discipline is normative); `rpi4-photoframe/src/` for
how decoders integrate.
**Deliverables:**
- `rpi4-llm-loader/`: no_std GGUF parser. Verus totality proofs: all
  reads in-bounds, no overflow in size arithmetic, every input either
  yields a validated `ModelDescriptor` (tensor table with checked
  offsets/sizes/quant types) or a clean error. This crate is the
  centerpiece — treat proof quality like `rpi4-input-protocol`.
- `rpi4-llm/`: arena-based llama2.c-style engine (single-threaded,
  integer/quantized kernels, fixed arenas sized from the verified
  descriptor — KV cache, activations, scratch). Hot loops are plain
  Rust; numerics property-tested on host against a reference
  implementation (llama2.c or candle on the same checkpoint).
- Product `llmdemo`: PD with an embedded stories15M-class checkpoint
  (GGUF, committed via Git LFS or fetched+pinned by SHA256 in
  `versions.mk` style), generates N tokens from a fixed prompt+seed.
- Host fuzz target for the loader (`cargo-fuzz`, run bounded in CI).
- **Signed execution receipts** (per review — this is what makes the
  artifact publishable): a **canonically encoded, versioned** structure
  `{receipt_version, verifier_nonce, input_digest, weights_digest
  (loader-computed), config+sampling params, output_digest}` with the
  **output bytes carried alongside** (a digest alone verifies
  nothing); a receipt module signs it with a build-injected test key in
  QEMU. The production key hierarchy (application receipt key,
  certified by the TPM AK, sealed to PCR policy, domain-separated from
  update/TLS/identity keys) is Wave 2 — do not sign receipts with an
  attestation key directly. Ship a host-side verifier
  (`llm-receipt-verify`) that checks the signature AND re-executes the
  model to confirm the output digest — determinism upgraded from
  tested property to *challenge protocol*.
**Acceptance (CI):** `qemu-llmdemo` job boots, generates ≥32 tokens,
prints `TOKENS SHA256 <hash>` — hash asserted in CI (determinism is a
*tested property*; the design doc enumerates what it's conditional on:
tokenizer, serialization, quant semantics, codegen, RNG, truncation,
config — pin all of them). Receipt printed on serial, extracted by the
CI job, and validated by `llm-receipt-verify` including re-execution.
Verus job passes on the loader. Fuzzer runs ≥60s in CI with zero
panics; a corpus of malformed GGUFs (truncated, oversized lengths,
overlapping tensors) is rejected cleanly in unit tests.
**Non-goals:** NEON optimization, models >100M params, sampling
strategies beyond greedy/temperature-with-fixed-seed, tokenizer
generality (whatever the checkpoint needs), weight capsules (composes
with WP-8 in Wave 2), TPM-backed receipt keys (Wave 2), any claim that
a receipt proves the model behaved well — it binds, it does not bless.

### WP-7: TPM transport trait refactor

**Goal:** make the TPM stack backend-portable per IC-3 (needed for
NitroTPM/cloud and keeps the CM4/PolarFire story unified).
**Read first:** `rpi4-tpm-boot/src/` (`spi.rs`, `slb9670.rs`),
`rpi4-tpm-pd/src/main.rs`.
**Deliverables:** `TpmTransport` trait (IC-3); SLB9670/SPI code
refactored to implement it with zero behavior change; `pcr.rs`,
`boot_chain.rs`, `attestation.rs` made transport-agnostic (they should
already be — enforce it with the type system); a `MockTransport` for
host tests replaying canned TPM 2.0 command/response pairs; host tests
for PCR-extend and quote-structure paths using the mock.
**Acceptance:** existing builds unchanged (`tpmtest` product still
compiles), new host tests pass, no public IPC surface change on
`rpi4-tpm-pd`.
**Non-goals:** TIS/CRB implementation, keystore features, hardware
validation.

### WP-8: Update capsule crate

**Goal:** IC-2 as a verified no_std library.
**Read first:** IC-2 (normative), design doc "Tier 2".
**Deliverables:** `update-capsule/` crate: header parser (Verus
totality, same bar as WP-6's loader), verification pipeline in the
IC-2 normative order, constant-time digest compare, ed25519 via a
formally verified implementation (libcrux preferred; if it won't build
no_std on the pinned nightly, use `ed25519-dalek` behind a
feature-gated seam and document the swap in the crate README), SHA-256
likewise. Key-generation + signing CLI for hosts
(`update-capsule-cli`, std) so other WPs and humans can mint test
capsules. Golden-file test vectors committed.
**Acceptance:** Verus passes on parser; host tests: valid capsule
accepted; each single-field corruption (magic, version, len, hash, sig,
rollback) rejected with a distinct error; fuzz target on the parser.
**Non-goals:** applying capsules (supervisor's job, Wave 2), transport,
key management/provisioning policy.

### WP-10: A/B update crash-safety model (TLA+)

**Goal:** the design doc's Tier-3 claim — "crash anywhere → boots old
or new image, never bricked" — model-checked.
**Deliverables:** `formal/ab-update/ABUpdate.tla` + `.cfg`: state
machine covering write-inactive-slot, verify, flag flip, first boot,
watchdog confirm/revert; a `Crash` action enabled at every step;
invariants `NeverBricked` (some signed image always bootable) and
`EventuallyConfirmed` (liveness under fair scheduling); a README
mapping model actions to the (future) implementation points and
documenting how to run TLC; CI job or documented manual run (if TLC in
CI is awkward, commit the TLC output log and a make target — do not
silently skip).
**Acceptance:** TLC exhausts the state space for a stated small
configuration (e.g. 2 slots, 3 crash budget) with both properties
holding; at least one *seeded bug* variant (flip flag before verify) is
shown to violate `NeverBricked` in the README, proving the model has
teeth.
**Non-goals:** modeling Tier-2 hot updates — but note (review): a
second crash model covering the Tier-2 ordering (TPM NV increment, PCR
extension, staging, slot write, restart) is a **blocking prerequisite
for WP-12**, not optional polish; U-Boot scripting.

### WP-11: Verus proofs for the network ring protocol

**Goal:** roadmap Phase 7, first item — bring
`rpi4-network-protocol` to the same proof standard as
`rpi4-input-protocol`.
**Deliverables:** proofs on the *existing* TX/RX ring code: index
bounds, SPSC ownership discipline, no entry reuse before release; kill
or justify the unused `ring_flags::IN_USE` (roadmap Phase 8 item) as
part of specifying ownership. Must not assume WP-4's generation field.
**Acceptance:** Verus passes; `cargo build` unchanged; roadmap Phase 7
box checked.
**Non-goals:** packet parser proofs (needs WP-1's stack to exist),
protocol redesign.

---

## Wave 2 — consumes Wave 1 via the contracts

Sketched here so Wave 1 agents know what they're feeding; each gets a
full spec when scheduled.

- **WP-12 Tier-2 hot update:** WP-3's lifecycle + WP-8 capsules, with
  the verifier/installer split into their own PDs per the design doc's
  supervisor decomposition (one-shot install authorizations; install
  sequence including D-cache clean + I-cache invalidate). CI: mint
  capsule with the WP-8 CLI, deliver via ring, watch `BOOT GEN` and a
  behavior change; rollback attempt rejected; wrong-slot and
  wrong-platform capsules rejected (IC-2 binding fields). TPM
  measurement mocked via WP-7's transport until a QEMU TPM is wired.
- **WP-13 HTTPS client PD:** smoltcp (WP-1) + `embedded-tls`; CI
  against a TLS server on the slirp host side with a pinned test cert.
  Uncredentialed traffic only — credentialed TLS lives in WP-14.
- **WP-14 Credential-use service (keystore PD):** WP-7 trait + WP-8
  crypto + its own TLS client for credentialed endpoints (the design
  doc's explicit TLS-ownership decision). Not a header stamper: enforces
  attenuated request capabilities — destination, operation, size,
  rate/cost ceilings, model allowlist, classification rules, audit log.
  The policy state machine is a Verus target; ghost-taint discipline on
  key bytes per the verification section.
- **WP-15 Agent-core PD + end-to-end demo:** rings + https + keystore;
  the "prompt on UART → Claude reply on HDMI" milestone (Phase C), with
  local-model routing to WP-6's PD as the private tier.
- **WP-16 Model weights as capsules:** WP-6 + WP-8 + WP-12 — measured
  model identity + receipts demo (attested *identity*, challengeable
  *provenance* — see design doc for the precise claim).
- **WP-17 Control-plane policy engine:** typed proposed actions from
  the model, deterministic authorization state machine (provenance
  labels, budgets, human confirmation via the trusted input/graphics
  path), Verus-verified. Per the design doc this is the most valuable
  proof target in the system.
- **WP-18 Wasm tool-runtime spike:** evaluate tools as WebAssembly
  modules (Veracruz-style: explicit imports, bounded memory, fuel
  limits, typed capability handles) vs. native slot blobs; feeds the
  tool-execution design before any native tool capsule ships.

---

## Tracking

| WP | Branch | PR | Status |
|---|---|---|---|
| WP-0 | — | — | not started |
| WP-1 | `agent/wp1-smoltcp-time` | #25 | DHCP/ICMP in QEMU; timer-driven renewal follow-up tracked |
| WP-3 | — | — | not started |
| WP-4 | — | — | not started |
| WP-5 | — | — | not started |
| WP-6 | — | — | not started |
| WP-7 | — | — | not started |
| WP-8 | — | — | not started |
| WP-10 | — | — | not started |
| WP-11 | — | — | not started |

(WP-2 and WP-9 numbers are reserved by the design doc's phasing —
folded into WP-1 and WP-13 respectively.)

## Orchestrator notes

- Wave 1 is 10 agents wide; all can *start* immediately. Shared mutable
  files are limited to the two protocol crates (WP-4/WP-11, both
  additive, merge order WP-4 first) and additive CI/build entries;
  WP-3 additionally consumes IC-1 by crate bump (legacy generation-0
  mode until WP-4 merges). Merge conflicts should be near-zero by
  construction; if an agent hits one, the ownership matrix (IC-5)
  decides who yields.
- Review order that de-risks fastest: WP-5 (cheap, guards everyone
  else's `.system` edits) → WP-4 → WP-3 → WP-6/WP-8 (the novel
  artifacts) → the rest.
- Every WP's acceptance is CI-checkable on purpose: an orchestrating
  agent can verify completion without trusting the worker agent's
  report — run the jobs.
