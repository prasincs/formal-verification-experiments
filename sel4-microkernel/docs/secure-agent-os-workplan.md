# Secure Agent OS — Execution Spec for Parallel Agents

Companion to [secure-agent-os.md](secure-agent-os.md) (the design; read
it first). This document decomposes the design into **work packages
(WPs)** that independent agents can execute concurrently. It fixes the
interface contracts, file ownership, and acceptance criteria up front so
that parallel work composes without coordination.

Execution model: one agent per WP, each on its own branch
(`agent/wp<N>-<slug>`), each landing as its own PR to `main`. Wave 1
WPs have **zero dependencies on each other**. Wave 2 WPs consume Wave 1
artifacts through the contracts in this file — not through reading Wave
1 branches.

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

### IC-1: Epoch ring header (WP-3 ⇄ WP-4 ⇄ everything using rings)

The existing ring header (`docs/device-isolation-architecture.md`) is:

```
0x000  u32  write_idx    (atomic, producer-owned)
0x004  u32  read_idx     (atomic, consumer-owned)
0x008  u32  capacity
0x00C  u32  padding      ← becomes `epoch`
0x010  ...  entries[]
```

- Offset `0x00C` becomes `epoch: u32` (atomic). Value `0` means
  "legacy/no supervisor" — existing images remain layout-compatible.
- Only the **supervisor** writes `epoch` (increments before restarting
  either endpoint, after zeroing `write_idx`/`read_idx`).
- Endpoints cache `last_seen_epoch`; on mismatch they reset local
  cursors/in-flight state, adopt the new epoch, and continue. Entries
  written under an older epoch are never consumed.
- New API surface (added, not replacing): `epoch()`, `check_epoch(&mut
  cached) -> EpochStatus { Unchanged, Reset }`.

### IC-2: Update capsule format (WP-8 ⇄ WP-3 ⇄ WP-6)

Little-endian, fixed-offset header, no variable-length fields before
the payload:

```
0x00  [u8;4]  magic = "SAOC"
0x04  u32     format_version = 1
0x08  u8      payload_type   (1 = pd-code, 2 = model-weights, 3 = config)
0x09  u8      target_slot    (slot PD id; 0 for whole-image)
0x0A  [u8;6]  reserved (zero)
0x10  u64     monotonic_version
0x18  u64     payload_len
0x20  [u8;32] payload_sha256
0x40  [u8;64] ed25519 signature over bytes [0x00..0x40) ++ payload
0x80  ...     payload
```

Verification order (normative): parse header (totality — reject, never
trust) → check `payload_len` bounds against buffer → hash payload →
compare `payload_sha256` (constant-time) → verify signature → check
`monotonic_version > stored version`. Only then is the payload
readable by anyone.

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
future second.

### IC-4: Supervisor demo system topology (WP-3)

Product `supdemo`, `supdemo.system`: PD `supervisor` (priority 200,
parent) with child PD `worker` (priority 100, `id="1"`). One shared
region `work_ring` (4KB, IC-1 layout) mapped into both; one channel
supervisor(id 1)↔worker(id 1). Worker exposes a "crash on demand"
input: a specific ring entry value causes a deliberate fault.

### IC-5: File ownership matrix

| WP | Owns (create/modify) | Read-only everywhere else |
|---|---|---|
| WP-1 | `rpi4-network/src/stack/`, `rpi4-network/src/time.rs`, `build-system/config/features/networking.mk` (additive) | yes |
| WP-3 | `rpi4-supervisor/` (new), `supdemo` product files, `rpi4-graphics/supdemo.system` or sibling | yes |
| WP-4 | `rpi4-input-protocol/` (additive), `rpi4-network-protocol/` (additive) | yes |
| WP-5 | `tools/system-check/` (new), one CI job | yes |
| WP-6 | `rpi4-llm/` (new), `rpi4-llm-loader/` (new), `llmdemo` product files | yes |
| WP-7 | `rpi4-tpm-boot/src/` (refactor within), `rpi4-tpm-pd/src/` | yes |
| WP-8 | `update-capsule/` (new crate) | yes |
| WP-10 | `formal/ab-update/` (new) | yes |
| WP-11 | `rpi4-network-protocol/` (proof additions only) | yes |

WP-4 and WP-11 both touch `rpi4-network-protocol`: WP-4 adds the epoch
field/API, WP-11 adds proofs to *existing* code. Both are additive;
merge order WP-4 → WP-11 preferred, and WP-11 must not assume epoch
exists.

---

## Wave 1 — fully parallel, no cross-dependencies

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
  logs fault info, zeroes ring indices, bumps epoch (IC-1), restarts
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

### WP-4: Epoch ring protocol + Verus proofs

**Goal:** restart-safe rings per IC-1, proven.
**Read first:** `rpi4-input-protocol/src/lib.rs` (the house proof
style), design doc "Tier 1".
**Deliverables:** epoch field + API in `rpi4-input-protocol` (additive;
offset `0x00C` per IC-1), mirrored in `rpi4-network-protocol`. Verus
invariants: index bounds preserved (existing), *entries consumed only
under matching epochs*, epoch adoption resets cursors, no
entry written under epoch N is readable under epoch M>N. Host unit
tests simulating producer/consumer/supervisor interleavings.
**Acceptance:** `cargo build` clean (macro strips), Verus verification
passes via the `verus/` harness, host tests cover: restart of producer,
restart of consumer, restart of both, epoch wraparound behavior
(document the chosen wrap policy).
**Non-goals:** changing existing entry formats; touching PD binaries
(WP-3 consumes this by crate bump, and ships even if WP-4 is late —
epoch 0 = legacy mode).

### WP-5: `.system` topology checker

**Goal:** machine-check the capability-topology claims (design doc
§"Check the `.system` files").
**Read first:** all `*.system` files (`rpi4-graphics/*.system`,
`rpi4-photoframe/photoframe.system`, `rpi4-network/netdemo.system`,
`microkit-hello/hello.system`), the isolation tables in
`docs/device-isolation-architecture.md`.
**Deliverables:** `tools/system-check/` (host Rust, std allowed):
parses a `.system` file into an access graph (PD → {region, perms,
kind}), then checks a sidecar `<name>.system.props.toml` declaring
properties:
```toml
[[shared_only]]           # exactly this set of regions shared between these PDs
pds = ["input", "graphics"]
regions = ["input_ring"]

[[exclusive]]             # region mapped into exactly one PD
region = "uart_regs"
pd = "input"

[[no_device_mmio]]        # PD maps no region with phys_addr
pd = "worker"
```
Write sidecars for every existing `.system` file, encoding the claims
the docs already make in prose.
**Acceptance (CI):** new job runs the checker over every
`.system`+sidecar pair; deliberately-broken fixture test proves the
checker fails when a mapping is widened. Checker itself has unit tests.
**Non-goals:** channel/IRQ policy language (v2), Verus-verifying the
checker (nice-to-have, not required).

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
**Acceptance (CI):** `qemu-llmdemo` job boots, generates ≥32 tokens,
prints `TOKENS SHA256 <hash>` — and the hash is asserted in CI
(determinism is a *tested property*, per the design doc's
attestable-inference claim). Verus job passes on the loader. Fuzzer
runs ≥60s in CI with zero panics; a corpus of malformed GGUFs
(truncated, oversized lengths, overlapping tensors) is rejected
cleanly in unit tests.
**Non-goals:** NEON optimization, models >100M params, sampling
strategies beyond greedy/temperature-with-fixed-seed, tokenizer
generality (whatever the checkpoint needs), weight capsules (that
composes with WP-8 in Wave 2).

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
**Non-goals:** modeling Tier-2 hot updates (separate model later),
U-Boot scripting.

### WP-11: Verus proofs for the network ring protocol

**Goal:** roadmap Phase 7, first item — bring
`rpi4-network-protocol` to the same proof standard as
`rpi4-input-protocol`.
**Deliverables:** proofs on the *existing* TX/RX ring code: index
bounds, SPSC ownership discipline, no entry reuse before release; kill
or justify the unused `ring_flags::IN_USE` (roadmap Phase 8 item) as
part of specifying ownership. Must not assume WP-4's epoch field.
**Acceptance:** Verus passes; `cargo build` unchanged; roadmap Phase 7
box checked.
**Non-goals:** packet parser proofs (needs WP-1's stack to exist),
protocol redesign.

---

## Wave 2 — consumes Wave 1 via the contracts

Sketched here so Wave 1 agents know what they're feeding; each gets a
full spec when scheduled.

- **WP-12 Tier-2 hot update:** supervisor (WP-3) + capsules (WP-8) +
  slot PD with shared executable region; CI: mint capsule v2 with the
  WP-8 CLI, deliver via ring, watch `BOOT GEN` and a
  behavior change; rollback attempt (v1 after v2) rejected. TPM
  measurement mocked via WP-7's transport until a QEMU TPM is wired.
- **WP-13 HTTPS client PD:** smoltcp (WP-1) + `embedded-tls`; CI
  against a TLS server on the slirp host side with a pinned test cert.
- **WP-14 Keystore PD:** WP-7 trait + WP-8 crypto; vault-proxy
  header-injection over the https PD (WP-13); ghost-taint discipline
  from the design doc's verification section.
- **WP-15 Agent-core PD + end-to-end demo:** rings + https + keystore;
  the "prompt on UART → Claude reply on HDMI" milestone (Phase C), with
  local-model routing to WP-6's PD as the private tier.
- **WP-16 Model weights as capsules:** WP-6 + WP-8 + WP-12 —
  attested model provenance demo.

---

## Tracking

| WP | Branch | PR | Status |
|---|---|---|---|
| WP-1 | — | — | not started |
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

- Wave 1 is 9 agents wide with no shared mutable files except the two
  protocol crates (WP-4/WP-11, both additive, merge order WP-4 first)
  and additive CI/build entries. Merge conflicts should be near-zero by
  construction; if an agent hits one, the ownership matrix (IC-5)
  decides who yields.
- Review order that de-risks fastest: WP-5 (cheap, guards everyone
  else's `.system` edits) → WP-4 → WP-3 → WP-6/WP-8 (the novel
  artifacts) → the rest.
- Every WP's acceptance is CI-checkable on purpose: an orchestrating
  agent can verify completion without trusting the worker agent's
  report — run the jobs.
