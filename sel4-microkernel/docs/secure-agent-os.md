# Secure Agent OS: a nanoclaw-class personal agent on seL4

Design sketch for evolving this repo from isolated demos (tvdemo,
photoframe, netdemo) into a **personal agent appliance**: a device that
runs a Claude-backed assistant where every component — channels, tools,
credentials, UI — lives in its own seL4 Protection Domain, and where PDs
can be **restarted and updated securely** at runtime.

Reference point: [nanoclaw](https://github.com/nanocoai/nanoclaw), a
~700-line personal agent on the Claude Agent SDK whose entire security
story is "each agent group runs in its own Docker container, and
credentials are brokered by a vault so agents never hold raw API keys."
That is exactly the shape of system this codebase can build with far
stronger guarantees: container isolation becomes capability-enforced PD
isolation on a formally verified kernel, and the vault becomes a
TPM-backed keystore PD.

## Why seL4 beats containers for this

Nanoclaw's threat model is real: an LLM agent executes untrusted
instructions (prompt injection via any message channel) and runs tools
with side effects. Its mitigation is Docker. Ours is stronger on every
axis:

| Property | nanoclaw (Docker) | This project (seL4 Microkit) |
|---|---|---|
| Isolation mechanism | Linux namespaces/cgroups (~30M LOC TCB) | Capabilities on a ~10K LOC kernel with machine-checked proofs |
| Escape surface | Kernel syscall surface, container runtime CVEs | Proven integrity/confidentiality; a PD *cannot* address memory it wasn't granted |
| Credential brokering | App-level vault process | Keystore PD; keys sealed to TPM PCRs, unmapped from every other PD |
| Least authority | Mount allowlists | Per-PD memory maps and channels declared in the `.system` file, enforced by the kernel |
| Supply-chain / update trust | `docker pull` | Signed update capsules, TPM-measured before activation |
| Attestation | None | TPM 2.0 quote over PCRs (already scaffolded in `rpi4-tpm-boot`) |

The LLM itself runs in the cloud (Claude API). The device is the
**trusted terminal and policy-enforcement point**: it owns the
credentials, the channels, the tools, and the human I/O path, and it is
the thing that must stay trustworthy when the model is fed hostile
input. That's precisely the part containers protect weakly and seL4
protects provably.

## Target architecture

```
                 ┌────────────────────────────────────────────────┐
                 │                seL4 (verified)                 │
                 └────────────────────────────────────────────────┘
   trusted ──────────────────────────────────────────────────────────────
                 ┌────────────┐      ┌───────────────┐
                 │ supervisor │──────│  keystore PD  │── SPI ── TPM 9670
                 │ PD (parent)│ PPC  │ (TPM broker + │
                 │ lifecycle, │      │  vault proxy) │
                 │ updates,   │      └───────┬───────┘
                 │ faults     │              │ inject Authorization
                 └─────┬──────┘              │ header, seal/unseal
        stop/restart/  │              ┌──────┴───────┐
        reload children│              │  https PD    │── ring ──┐
   ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┼ ─ ─ ─ ─ ─ ─ ─│ (smoltcp +   │          │
   semi-trusted        │              │  TLS client) │   ┌──────┴─────┐
                 ┌─────┴──────┐       └──────────────┘   │ network PD │─ GENET/
                 │ agent-core │  rings   ▲               │ (existing) │  virtio
                 │ PD (conv.  │──────────┘               └────────────┘
                 │ loop, no   │
                 │ keys!)     │───────────┬─────────────┬─────────────┐
                 └────────────┘           │             │             │
   ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─│─ ─ ─ ─ ─ ─ ─│─ ─ ─ ─ ─ ─ ─│─ ─
   untrusted / restartable         ┌──────┴─────┐ ┌─────┴─────┐ ┌─────┴─────┐
   (child PDs of supervisor)       │ tool slot  │ │ tool slot │ │ channel   │
                                   │ PD #1      │ │ PD #2     │ │ PD (e.g.  │
                                   └────────────┘ └───────────┘ │ email)    │
                                                                └───────────┘
                 ┌────────────┐  ┌──────────────┐  ┌────────────┐
   device I/O    │ input PD   │  │ graphics PD  │  │ storage PD │
   (existing)    │ (UART/HID) │  │ (HDMI)       │  │ (SD/flash) │
                 └────────────┘  └──────────────┘  └────────────┘
```

Mapping nanoclaw concepts onto PDs:

| nanoclaw | Here | Status |
|---|---|---|
| Agent group container | Agent/tool **slot PD** (child of supervisor) | new |
| OneCLI Agent Vault | **Keystore PD**: extends `rpi4-tpm-pd`'s broker pattern; holds API keys sealed to PCRs, injects auth headers so agent PDs never see raw keys | TPM broker exists (`rpi4-tpm-pd/src/main.rs`) |
| Channel adapters (WhatsApp, …) | **Channel PDs** behind the https PD | new |
| Memory (CLAUDE.md, notes) | **Storage PD** owning the SD card | new (no storage driver yet) |
| Scheduled jobs | **Timer PD** (generic timer, `CNTVCT_EL0`) | new (also needed by smoltcp — see networking roadmap Phase 4) |
| Claude Agent SDK loop | **Agent-core PD**: conversation state machine, tool dispatch | new |
| Host bash / tools | **Tool slot PDs**: pre-declared generic PDs the supervisor loads code into | new |

The key architectural rule carried over from nanoclaw, but enforced by
the kernel instead of by convention: **the agent-core PD composes API
requests but holds no credentials**. Requests flow through the keystore
PD, which attaches the `Authorization` header inside the TLS session it
brokers. A fully prompt-injected agent-core PD can emit garbage requests
but cannot exfiltrate the key, read another PD's memory, or touch a
device it wasn't mapped.

## PD restart and update: the missing mechanism

Today every system in the repo is fully static: the Microkit tool bakes
all PD ELFs and capability mappings into one `loader.img`, and nothing
can be reloaded at runtime. Grep confirms no dynamic-lifecycle work
exists anywhere. But the repo pins **Microkit SDK 2.1.0**
(`build-system/config/versions.mk`), and Microkit ≥1.4 supports exactly
the primitive we need: **hierarchical protection domains**. A parent PD:

- receives its children's **faults** (its `fault` entry point is
  invoked instead of the system faulting),
- can call **`microkit_pd_stop(child)`** and
  **`microkit_pd_restart(child, entry_point)`**.

That turns the supervisor PD into a capability-scoped init system.
Constraints to design within: PDs are still declared statically (max
63 per system), so "dynamic" agents are **pre-declared slot PDs**; and
Microkit gives the parent no direct handle on the child's original
program image, so hot-reload uses a shared executable region (below).

### Tier 1: restart (crash recovery / watchdog)

Supervisor catches a child fault (or a missed heartbeat over a
notification channel), stops the child, reinitializes shared state, and
restarts it at its entry point.

The subtle part is the **ring buffers**. Every IPC path in this repo is
an SPSC ring (`rpi4-input-protocol`, `rpi4-network-protocol`,
`rpi4-photo-protocol`) whose indices live in shared memory. A restarted
producer that re-inits `write_idx = 0` while its peer holds
`read_idx = 700` silently corrupts the stream. So restartability needs a
small protocol extension, which is also a natural next Verus target:

- add an **epoch/generation counter** to the ring header, bumped by the
  supervisor on every restart of either endpoint;
- peers snapshot the epoch on each operation; on mismatch they reset
  their local index and drop in-flight entries;
- Verus invariant: entries are only consumed when producer and consumer
  epochs agree (extends the existing verified SPSC discipline in
  `rpi4-input-protocol/src/lib.rs`).

This makes "kill any PD at any time" a safe operation — which is worth
having even before agents exist (network driver watchdog, decoder PD
crash containment for photoframe, which its `.system` file already
anticipates).

### Tier 2: hot code update (per-PD, signed + measured)

For agent/tool slot PDs, updates without reboot:

1. Slot PDs are declared with their executable region as an explicit
   `memory_region` mapped `rx` into the slot and `rw` into the
   supervisor (Microkit can't hand a parent the child's *original*
   program image, so the slot's real payload lives in this shared
   region; the baked-in slot ELF is just a trampoline that jumps into
   it, or the supervisor restarts the slot directly at the region's
   entry point).
2. New code arrives as an **update capsule**: `{blob, version,
   ed25519 signature}` fetched via the https PD or loaded from
   storage.
3. Supervisor verifies the signature against a pinned public key,
   asks the keystore PD to **extend a PCR with the blob digest**
   (append-only measured-update log — the event-log machinery in
   `rpi4-tpm-boot/src/boot_chain.rs` and `attestation.rs` is exactly
   this), and enforces version monotonicity (anti-rollback counter,
   sealable in a TPM NV index).
4. `microkit_pd_stop(slot)` → write blob → bump ring epochs →
   `microkit_pd_restart(slot, entry)`.

Blobs must be position-independent or linked to the slot's fixed region
base; slot PDs get a deliberately generic, minimal capability set
(rings to agent-core, nothing else), which is what makes running
freshly-downloaded code in them acceptable.

5. A remote party (your phone, a home server) can then demand a **TPM
   quote** over the boot + update PCRs and know exactly which agent
   code the device is running — attestation the container world simply
   doesn't have.

### Tier 3: whole-image A/B update (fallback and TCB updates)

Supervisor/keystore/kernel changes can't hot-swap themselves. Standard
embedded answer: two image slots on the SD card, U-Boot (already in the
boot chain, pinned in `versions.mk`) picks the active slot via an
environment flag, new images are signature-verified and measured before
the flag flips, and a boot-success watchdog flips it back on failure.
This is boring, robust, and should land *first* — Tier 2 is an
optimization on top of it.

## Where formal verification pays off

The repo's layered model (seL4 proofs = runtime enforcement, Verus =
compile-time protocol correctness, specs = design intent —
`docs/device-isolation-architecture.md`) extends directly to the agent
OS. The discipline: **verify the components where one bug defeats the
whole design; consume already-verified artifacts for crypto; model-check
the distributed/crash behavior; test everything else.** Verus's `verus!`
macro strips to plain Rust under `cargo build` (the trick documented in
`verus/README.md`), so verified code costs nothing at runtime.

| Component | Property | Tool | Why it's worth it |
|---|---|---|---|
| Supervisor lifecycle state machine | verify-before-write, measure-before-execute, anti-rollback, no TOCTOU | Verus | a bug here *is* "run unsigned code" |
| Epoch ring protocol | SPSC + restart safety | Verus (extends `rpi4-input-protocol`) | a bug here corrupts IPC on every restart |
| Update capsule / TPM / HTTP parsers | total parsing, no OOB/overflow | Verus + fuzzing | trust-boundary input, classic exploit surface |
| Keystore buffer discipline | key bytes never written to shared regions | Verus ghost-taint + seL4 mappings | the credential-isolation claim, made checkable |
| `.system` capability topology | access-graph matches the documented isolation claims | small checker tool in CI | turns the docs' hand-written tables into machine-checked facts |
| A/B update + watchdog protocol | crash anywhere → boots old or new image, never bricked | TLA+ | power-loss interleavings exceed what testing finds |
| ed25519, SHA-256 | correctness, constant time | consume HACL\*/libcrux, fiat-crypto | never hand-verify crypto |
| seL4 kernel | integrity/confidentiality | already proven (Isabelle/HOL) | the foundation everything above stands on |

### 1. The supervisor's update state machine (highest value)

The Tier-2 flow — stop → verify → measure → write → restart — is a
small state machine where every wrong transition is a security failure.
Verus proofs worth writing, in the style of the existing ring proofs:

- **Verify-before-write:** code bytes reach a slot's executable region
  only from a PD-private staging buffer whose digest was
  signature-checked, with no mutation between hash and copy (kills the
  TOCTOU where a peer PD rewrites the blob after verification — provable
  because the staging buffer is *not* shared, which the `.system`
  checker below confirms independently).
- **Measure-before-execute:** `microkit_pd_restart` is unreachable in
  any execution path where the PCR-extend for that blob hasn't
  completed. The attestation story is only as good as this invariant.
- **Anti-rollback monotonicity:** accepted version numbers strictly
  increase, matching the TPM NV counter.
- **Panic-freedom** of the whole supervisor: a panicking supervisor
  orphans every child PD, so proving absence of panics (Verus does this
  naturally — all `unwrap`/index sites discharge or fail the build) is
  an availability property, not a nicety.

### 2. Restart-safe rings (direct extension of existing work)

`rpi4-input-protocol` already proves index bounds and SPSC discipline.
The epoch extension adds one invariant family: *an entry is consumed
only when producer and consumer epochs agree*, so a restarted endpoint
can never cause its peer to read stale or uninitialized slots. Same
proof style, same crate layout — and it should land for the network and
photo protocol crates at the same time (both are currently unverified
copies of the input crate's shape; `docs/networking-roadmap.md` Phase 7
already calls for this).

### 3. Parsers at trust boundaries

Everything that parses bytes from a less-trusted domain gets the
decoder-PD treatment the photoframe docs already argue for
(`docs/secure-photo-frame-architecture.md`), plus Verus totality proofs:
update capsule headers (length fields, offsets — reject, never trust),
TPM 2.0 command/response marshalling in the keystore PD, and the
HTTP/SSE chunking layer in the https PD. Verus proves no
out-of-bounds, no integer overflow, and that every input is either
fully parsed or cleanly rejected. Complement with `cargo-fuzz` on the
host builds (the `rpi4-photoframe-tests` pattern) — proofs rule out
memory errors, fuzzing catches logic errors the spec didn't anticipate.

### 4. The keystore claim, made checkable

"Agent PDs never see raw keys" is the design's marquee property. It
decomposes into two independently checkable halves:

- **seL4/config half:** the key-material region is mapped into the
  keystore PD only. That's a fact about the `.system` file — see below.
- **Code half:** within the keystore PD, key bytes flow only into the
  TLS session/sealing sinks, never into ring-buffer writes. Verus can
  enforce this with a ghost taint on secret buffers (secret-typed bytes
  have no path to functions that write shared regions) — the same
  spec-function style as the existing `*_pd_can_access` specs, but
  attached to real code. Full information-flow verification is
  research-grade; this "typed sinks" discipline is practical and
  catches the realistic bugs (logging a key, copying it into a
  response struct).

### 5. Check the `.system` files, not just the code

Every isolation argument in this repo ultimately rests on hand-written
XML — which PD maps which region with which perms. Today those claims
live in doc tables. A small CI tool (plain Rust, or Verus-verified for
sport) should parse each `.system` file, build the access graph, and
assert the security-critical facts: *the only region shared between
input and graphics is `input_ring`; the keystore key region has exactly
one mapping; no slot PD maps any device MMIO; supervisor staging
buffers are unshared.* This is a lightweight, Microkit-scale version of
what CapDL does for full seL4 systems, it costs a day, and it converts
the architecture docs from prose into regression-checked properties.
It also guards the failure mode most likely in practice: a future
`.system` edit quietly widening a mapping.

### 6. Crash safety of A/B updates: model checking, not proof

The Tier-3 protocol (write inactive slot → verify → flip U-Boot flag →
boot → watchdog confirms or reverts) has its interesting bugs in
*interleavings*: power loss between flag-flip and first successful
boot, watchdog racing the confirmation write, double-failure paths. A
TLA+ model with a crash action enabled at every step, checking "always
eventually boots a signed image; never bricks," is a few hundred lines
and is the right tool — SMT-backed code verifiers are poor at this,
model checkers excel at it.

### 7. Consume verified crypto; don't write it

Capsule signatures (ed25519) and PCR digests (SHA-256) should come from
formally verified implementations — HACL\*/libcrux (proven in F\*) or
fiat-crypto field arithmetic — rather than being proven in Verus, which
is the wrong tool for constant-time and algebraic correctness. The
constant-time PCR compare already claimed behind `rpi4-tpm-boot`'s
`--features verus` gate should graduate to a libcrux-backed primitive.
Kani (bounded model checking on plain Rust) is the pragmatic middle
ground for the bit-twiddling driver code — MMIO register manipulation
in the GENET/TPM drivers — where SMT-style Verus proofs get painful.

### What this buys, honestly

The composed claim becomes: *if seL4's proofs hold (Isabelle/HOL,
covering the RV64 and AArch64 kernels we deploy on), and the `.system`
topology passes the checker, and the supervisor/keystore/ring proofs
discharge, then a fully compromised agent, tool, or channel PD cannot
read credentials, corrupt another PD, persist across a signed update,
or evade attestation.* What remains unverified and tested-only:
smoltcp, the TLS implementation (mitigated by verified crypto
underneath and cert pinning), device drivers (mitigated by
capability-scoped blast radius), and the LLM's behavior itself
(mitigated by the whole architecture — that's the point).

## Honest platform caveats

- **RPi4 secure boot exists but is opt-in and rooted in closed
  firmware.** The stock boot flow verifies nothing, so out of the box
  the TPM gives *measured* boot only — tampering is detectable via
  attestation and PCR-sealed secrets become unrecoverable, but not
  prevented. RPi4/CM4 can be upgraded to *verified* boot by fusing a
  customer public-key hash into SoC OTP, after which the EEPROM
  bootloader only loads a customer-signed `boot.img` (see
  [Deployment targets](#deployment-targets-with-secure-boot) below).
  Even then the chain roots in the closed VideoCore boot ROM and
  Raspberry Pi-signed bootloader — the trust gap the repo already
  acknowledges (`rpi4-spi-display/README.md`). Document it, don't
  oversell it.
- **The LLM is remote.** Agent quality/behavior is Anthropic's model +
  our prompts; the OS guarantees are about *containment of effects*,
  credentials, and code integrity — which is the right division of
  labor.
- **63-PD limit** bounds the number of slots; fine for a personal
  agent (a handful of tools + channels), just pre-plan the slot count
  per image.

## Deployment targets with secure boot

The update/attestation design above assumes the platform can anchor a
chain of trust. Four realistic paths, in increasing order of porting
effort.

**The repo's existing TPM stack is the portable layer across all of
them.** `rpi4-tpm-boot` already separates concerns cleanly: `pcr.rs`,
`boot_chain.rs`, and `attestation.rs` (PCR banks, measurement chain,
quote/event-log structures) are pure TPM 2.0 logic with no platform
dependency; only `spi.rs` + `slb9670.rs` are transport/chip-specific.
Likewise `rpi4-tpm-pd`'s IPC surface (`Init`, `PcrExtend`, `PcrRead`,
`GetRandom`, …) is what client PDs program against, not the chip. So
each target below keeps the measurement/attestation/keystore design
unchanged and swaps at most the transport. Secure boot doesn't replace
this work — it **completes** it: verified boot prevents tampered images
from running at all, while the existing measured-boot chain (PCR0
firmware … PCR3 PD images … PCR7 policy, per the `rpi4-tpm-boot`
README) is what proves the running state to a remote party and gates
unsealing of the API keys. Prevention and attestation are different
jobs; we need both, and we already have half.

### 1. Stay on Pi: RPi4 / CM4 with OTP-fused signed boot

Raspberry Pi 4 and CM4 support real secure boot: `recovery.bin` fuses
the SHA-256 of a customer RSA-2048 public key into SoC OTP
(irreversible), after which the EEPROM bootloader refuses any EEPROM
config or `boot.img` not signed with that key
([docs](https://github.com/raspberrypi/usbboot/blob/master/docs/secure-boot.md),
[RPi 4 boot security whitepaper](https://pip.raspberrypi.com/categories/1260-security/documents/RP-004651-WP/Raspberry-Pi-4-Boot-Security.pdf)).
`boot.img` is a FAT ramdisk containing the firmware + our U-Boot +
`loader.img` — i.e. the whole Microkit image rides inside the signed
artifact with **zero code changes** to this repo.

- Fit: keeps the existing BCM2711 drivers, the SLB9670 SPI TPM (same
  GPIO 7–11 wiring, `spi.rs`/`slb9670.rs` unchanged), and the build
  system. The Tier-3 A/B update flips between two *signed* `boot.img`
  files; the supervisor's capsule signature check stays as the Tier-2
  layer.
- Prefer **CM4 + carrier** over the 4B for deployment: eMMC instead of
  a swappable SD card, and `rpiboot`-gated EEPROM provisioning.
- Residual trust: closed VideoCore ROM and RPi-signed bootloader stages
  sit below our chain; RSA-2048-only; key revocation is limited. RPi5
  has the same facility but seL4 has no BCM2712 port yet.

### 2. ARM derivative with industrial secure boot: NXP i.MX8M

Microkit/seL4 already support i.MX8M-family boards (e.g. `imx8mm_evk`,
`imx8mq_evk` — see [Microkit supported platforms](https://docs.sel4.systems/projects/microkit/platforms.html)),
and NXP's **HAB (High Assurance Boot)** is the mature embedded answer:
the on-die ROM verifies the first-stage image against key hashes in
eFuses, with proper key revocation and a field-proven provisioning
flow. This is the "same architecture, better silicon" move:

- Port cost: new platform `.mk` + device drivers (UART, ENET instead of
  GENET, eMMC); the PD architecture, protocols, and supervisor design
  carry over unchanged. AArch64 target JSON already exists.
- The CAAM crypto engine can complement or replace the discrete SPI
  TPM; keeping the `rpi4-tpm-pd` broker interface stable means client
  PDs don't care which backend signs quotes.

### 3. RISC-V: PolarFire SoC Icicle (strongest end-to-end story)

seL4's functional-correctness proofs cover RV64, and the
[Microchip PolarFire SoC Icicle Kit is a supported seL4 platform](https://docs.sel4.systems/Hardware/polarfire.html)
with an ecosystem already using it for exactly this trusted-base role
([DornerWorks](https://www.dornerworks.com/blog/sel4-on-polarfire-soc/)).
PolarFire SoC brings its own hardware root of trust: immutable boot
ROM-equivalent secure boot, device certificates, and tamper features —
no closed application-processor firmware in the chain at all. Combined
with the verified kernel this is the maximal "provable stack" target.

- The repo already builds and CI-tests `qemu-riscv64`
  (`microkit-hello`), so the toolchain path exists today; porting means
  platform bring-up (HSS boot handoff, UART, MACB Ethernet) rather than
  new architecture work.
- The Icicle Kit exposes SPI on its Pi-compatible header, so the
  discrete SLB9670 and the existing driver stack carry over here too —
  attestation code identical across ARM and RISC-V deployments.
- Cheaper RISC-V boards (Star64/VisionFive 2, JH7110) are
  seL4-supported but have a much weaker/poorly documented secure-boot
  story — fine for development, not for the trust anchor.

### 4. Cloud: seL4 as a UEFI guest with vTPM

For an always-on personal agent without hardware on a shelf,
[EC2 supports UEFI Secure Boot and NitroTPM (TPM 2.0)](https://aws.amazon.com/blogs/aws/amazon-ec2-now-supports-nitrotpm-and-uefi-secure-boot/):
you can enroll **your own PK/KEK/db keys** in the instance's UEFI
variable store (via `--uefi-data` at image registration), sign your own
bootloader, and get measured boot + PCR-sealed secrets + remote
attestation from the vTPM
([deep dive](https://aws.amazon.com/blogs/compute/deep-dive-into-nitrotpm-and-uefi-secure-boot-support-in-amazon-ec2/)).
Azure Trusted Launch and GCP Shielded VMs offer the equivalent
(GCP also accepts custom secure-boot certificates on custom images).

- Path here: the repo's `sel4-x86_64/` rootserver approach (Microkit
  has no x86_64 yet — it's on the roadmap), chain-loaded from a signed
  GRUB/multiboot2 EFI binary; or run the AArch64 Microkit image under
  QEMU/KVM on a metal instance as a staging environment.
- Driver deltas are small and generic: virtio-net (already CI-proven),
  virtio-blk for storage, and a **TIS/CRB MMIO transport** for the TPM
  driver in place of the SLB9670 SPI transport — worth structuring
  `rpi4-tpm-pd` behind a transport trait now so the broker interface
  is backend-agnostic.
- Trust model shifts: the hypervisor (AWS/Azure/GCP) is inside the TCB.
  That's weaker than PolarFire, stronger than most people's home
  network, and the same CI images (`qemu-netdemo` pattern with OVMF +
  swtpm) double as the local test rig for the whole secure-boot +
  vTPM flow before any cloud deployment.

### Recommendation

Near-term: **CM4 with OTP signed boot** — it upgrades the exact
hardware this repo runs on from measured-only to verified boot with no
code changes, and the A/B update design slots straight into signed
`boot.img` pairs. Add **QEMU + OVMF + swtpm** CI early since it
exercises secure boot + TPM logic on every commit. Mid-term, **i.MX8MM**
if the project wants deployable ARM hardware with serious provisioning,
or **PolarFire Icicle** if the goal is the maximal formal story
(verified kernel on RV64 + open hardware root of trust). Cloud is the
low-friction way to run the agent 24/7 once the x86_64 or
virtio-AArch64 path lands.

## Local inference: the model PD

Can the device run its own LLM — llama.cpp-style — inside a PD? Yes,
with eyes open about hardware, and the architecture gets genuinely
interesting security properties out of it. Two routes:

### Route A: a VM PD running Linux + llama.cpp (pragmatic)

llama.cpp wants libc, libstdc++, pthreads, and mmap — a bare-metal
Microkit port is a large, thankless effort. The standard seL4 answer
for "big legacy software" applies instead: **run it in a virtual
machine that is itself a child of a PD.** Microkit supports this
natively — `<virtual_machine>` elements with vCPUs, where the parent
PD is the VMM and receives all guest faults
([manual](https://github.com/seL4/microkit/blob/main/docs/manual.md)),
with [libvmm](https://github.com/au-ts/libvmm) as the AArch64 VMM
library (in development; boots Linux guests; the LionsOS pattern).

- **Isolation story holds.** The guest Linux is inside the model PD's
  box, not inside the system's TCB: it gets its own RAM region, zero
  device mappings, and talks to agent-core over the same shared-memory
  rings (or virtio-console) as any other PD. A kernel exploit *inside*
  the model VM yields exactly the model PD's capabilities — nothing.
- **Threads and cores.** Microkit PDs/vCPUs take a `cpu` attribute, so
  the model VM gets cores 2–3 (llama.cpp threads inside the guest)
  while input/graphics/network keep cores 0–1 and their latency.
- **Path:** bring it up on `qemu_virt_aarch64` first (libvmm's
  best-supported target, and it slots into the existing QEMU CI
  pattern); RPi4 hypervisor-mode support (EL2 + GIC-400 quirks) needs
  validation before promising it on the Pi.

### Route B: a native no_std inference PD (purist)

A llama2.c-scale engine — a few hundred lines of transformer inference
— ports cleanly to a no_std Rust PD: no libc, no threads required,
and **pre-allocated arenas instead of malloc**, which is exactly the
allocation discipline `docs/decoder-allocation-security.md` already
prescribes for the image decoders. Weights load from a big
Microkit memory region; quantized matmuls use NEON. Realistic for
sub-1.5B-parameter models; forget 7B. Bonus: the GGUF/weights parser
becomes a Verus totality target like every other trust-boundary parser
— which matters, because malformed-GGUF parsing bugs in llama.cpp are
real, published CVEs, and "model file" is just "malicious media file"
wearing a lab coat. The photoframe decoder threat model transfers
verbatim.

### What the security architecture buys you here

1. **Attested model provenance.** Weights ship as Tier-2 signed
   capsules, measured into a PCR like any code blob. The device can
   *prove to a remote party which model produced an answer* — a
   property no mainstream local-inference stack offers.
2. **Contained model supply chain.** A poisoned or malformed model file
   detonates inside a PD with no device caps and no credentials —
   same blast-radius argument as the photo decoder, now covering the
   scariest new input format of the decade.
3. **A private tier.** Routing becomes a policy decision in agent-core:
   sensitive prompts (health, finances, home presence) go to the local
   model and never leave the device; heavy reasoning goes to Claude;
   the local model triages, summarizes notifications, and keeps the
   agent minimally functional offline.

### Hardware honesty

RPi4's 4×A72 gives TinyLlama-class models (0.5–1.5B, Q4) at a few
tokens/second and nothing usable beyond ~3B — a triage/private tier,
not the main brain; the 8GB variant is effectively required. The
deployment targets change the math: RPi5/CM5 roughly triples it (once
an seL4 BCM2712 port exists), i.MX8M is comparable to RPi4, and the
cloud path gets AVX2-class throughput but pays the "hypervisor in TCB"
tax already noted. The two-tier design (local = private + fallback,
cloud = smart) is honest about all of these.

Sequencing: this is a post-Phase-C feature (it needs the supervisor,
rings, and update pipeline), and Route A's QEMU-first bring-up means it
can mature entirely in CI before touching hardware.

## Gap analysis

What exists and carries over directly:

| Building block | Where | State |
|---|---|---|
| PD isolation pattern + verified SPSC rings | `tvdemo-input.system`, `rpi4-input-protocol` | shipping; input protocol is the one Verus-verified crate actually linked into PD ELFs |
| Untrusted-component containment | `photoframe.system`, `docs/secure-photo-frame-architecture.md` | shipping (decoder threat model = tool-PD threat model) |
| Hardware-broker service PD w/ PPC | `tpm-boot.system` (`pp="true"`), `rpi4-tpm-pd` | shipping pattern → keystore PD |
| Measured boot / attestation structures | `rpi4-tpm-boot` (`pcr.rs`, `boot_chain.rs`, `attestation.rs`) | scaffolded, needs hardware validation |
| Network PD + drivers | `rpi4-network` | virtio: CI-proven end-to-end; GENET: code-complete, unvalidated on silicon |
| Multi-PD build machinery | `build-system/config/products/*.mk` | shipping (hand-wired per product) |

What's missing, roughly in dependency order:

1. **IP stack** — smoltcp over `NetworkDriver` (already Phase 4 of
   `docs/networking-roadmap.md`), plus the **timer PD** it needs.
2. **TLS + HTTPS client PD** — `embedded-tls` or `rustls` (no_std) to
   reach `api.anthropic.com`; cert pinning keeps the trust store tiny.
3. **Supervisor PD + child-PD conversion** — first use of Microkit
   hierarchical PDs in the repo; restart demo killable in QEMU CI.
4. **Ring epoch protocol** — restart-safe rings, Verus-verified.
5. **Storage PD** — SD/eMMC driver (needed for photos anyway, per the
   photoframe 5-PD target design).
6. **Update pipeline** — capsule format, ed25519 verify, A/B images,
   then Tier-2 hot slots.
7. **Agent-core PD + keystore vault-proxy** — the actual agent.
8. **Channels / tools / scheduler** — the nanoclaw feature surface.

## Suggested phasing

Each phase is independently demoable and CI-testable in QEMU (the
existing `qemu-netdemo` job is the template):

- **Phase A — network becomes useful:** smoltcp + timer PD; DHCP +
  ICMP echo against QEMU slirp in CI.
- **Phase B — supervised PDs:** supervisor parent PD; convert netclient
  to a child; CI test that force-faults the child and asserts recovery
  with epoch-reset rings. *(Independent of Phase A; can run first.)*
- **Phase C — talk to Claude:** https PD, keystore header-injection
  proxy (software keys first, TPM sealing later); minimal agent-core
  PD; end-to-end demo: prompt typed on UART/keyboard → response on
  HDMI. This is the "it's alive" milestone.
- **Phase D — secure updates:** A/B image update with signature check;
  then Tier-2 signed hot-swap of one tool slot PD, measured into a PCR.
- **Phase E — agent surface:** storage PD (conversation memory),
  scheduler in supervisor, first real tool slots and a channel PD.
- **Verification thread (continuous):** Verus on the network ring, the
  epoch protocol, capsule parsing, and the supervisor's lifecycle state
  machine (a small state machine over stop/verify/measure/write/restart
  is an ideal Verus target — a bug there is exactly a "load unsigned
  code" bug).

## Bottom line

Nothing in the nanoclaw model is out of reach; the repo has been
unknowingly building its substrate. The demos established the isolation
grammar (PDs + verified rings + broker PDs + measured boot); the agent
OS is that grammar applied to a new vocabulary: supervisor, keystore,
https, agent-core, slots. The genuinely new kernel-adjacent work is
small and well-supported by the pinned SDK (hierarchical PDs). The
long-pole engineering is unglamorous: TCP, TLS, storage, and update
plumbing — all of it CI-provable in QEMU before ever touching the Pi.
