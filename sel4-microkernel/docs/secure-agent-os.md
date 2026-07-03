# High-Assurance Agent Appliance: Design RFC

**Status: RFC / design exploration — nothing here is implemented.**
Claims are validated only where a cited artifact exists in this repo.

This revision incorporates an external design review (2026-07) whose
central criticism is accepted in full: the original draft slid between
four distinct guarantees — (1) formally verified isolation, (2)
memory-safe/verified component code, (3) measured/attested software
identity, and (4) semantically correct agent behavior — of which only
the first is substantially inherited from seL4; the rest require
additional proofs, trusted components, protocols, and explicit
threat-model assumptions. The defensible framing, used throughout this
revision, is: **a capability-secure, attestable agent appliance with
selectively verified control-plane and inference-boundary components**
— not a "verified agent OS." See the [assurance case](#assurance-case)
for the claim-by-claim status.

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

## Why seL4 — and what containers still do better

Nanoclaw's threat model is real: an LLM agent executes untrusted
instructions (prompt injection via any message channel) and runs tools
with side effects. Its mitigation is Docker. seL4 provides a stronger
*isolation foundation* under a smaller, explicit, partially verified
TCB — which is a narrower statement than "better on every axis":

| Property | nanoclaw (Docker) | This project (seL4 Microkit) |
|---|---|---|
| Isolation mechanism | Linux namespaces/cgroups (~30M LOC TCB) | Capabilities on a ~10K LOC kernel with machine-checked proofs |
| Escape surface | Kernel syscall surface, container runtime CVEs | Proven integrity/confidentiality; a PD *cannot* address memory it wasn't granted |
| Credential brokering | App-level vault process | Keystore PD; keys sealed to TPM PCRs, unmapped from every other PD |
| Least authority | Mount allowlists | Per-PD memory maps and channels declared in the `.system` file, enforced by the kernel |
| Supply-chain / update trust | `docker pull` | Signed update capsules, TPM-measured before activation |
| Attestation | None | TPM 2.0 quote over PCRs (already scaffolded in `rpi4-tpm-boot`) |

For fairness: containers win on ecosystem, hardware and accelerator
support, observability, and update tooling, and a confidential-VM/TEE
deployment of a mature Linux agent stack is the faster route to a
usable product (see [Related work](#related-work)). The bet this
project makes is that for a *personal trust anchor* — the thing holding
your credentials — a small explicit TCB beats a rich ecosystem.

### Scope of the seL4 guarantee (read before quoting the table)

What seL4's proofs give us is **verified kernel-mediated spatial
isolation, under the selected proof assumptions and configuration** —
not a verified appliance. Outside the proofs and therefore inside our
trust-by-other-means budget: the Microkit tool and loader, boot
firmware and U-Boot, device-tree/platform initialization, every driver
we write, DMA-capable devices (next subsection), TPM firmware and the
SPI bus to it, the Rust toolchain, timing/covert channels (a separate,
ongoing research area for seL4), the specific SMP configuration (seL4's
functional-correctness story is strongest single-core; multicore
verification is still maturing), and physical attackers. Each row of
the [assurance case](#assurance-case) says which of these it leans on.

### DMA: the honest hole in Pi-class hardware

A device that can DMA arbitrarily defeats MMU-based isolation from
below, no matter what the kernel proves. BCM2711 has **no IOMMU/SMMU
in front of the bus masters we use** (GENET Ethernet, USB), so a
compromised network driver PD programming its NIC — or a malicious
device — can in principle read or write any physical memory, including
the keystore's. The repo's existing fixed-phys DMA carve-outs
(`net_dma` at `0x3e700000` in `tvdemo-network.system`) constrain an
*honest* driver's buffers; they do not constrain a *malicious*
descriptor. Consequences for this design:

- On RPi4/CM4, the network driver PD + NIC must be treated as
  potentially having system-wide write authority. Mitigations are
  softening, not solving: keep the driver minimal and memory-safe
  (Rust), keep secrets sealed in the TPM rather than resident in RAM
  where possible, and never claim DMA-immune isolation on this board.
- Platform choice changes this materially: the cloud targets have
  virtio devices mediated by the host IOMMU, i.MX8M and PolarFire have
  varying degrees of bus-master control, and any future
  accelerator-based inference path must answer "who configures the
  IOMMU, and is that configuration measured?" before it touches the
  architecture.
- The `.system` topology checker (verification section) should flag
  every PD that owns a DMA-capable device as a distinguished trust
  class, so the docs can never quietly forget this.

The LLM itself runs in the cloud (Claude API). The device is the
**trusted terminal and policy-enforcement point**: it owns the
credentials, the channels, the tools, and the human I/O path, and it is
the thing that must stay trustworthy when the model is fed hostile
input. That's the part containers protect weakly and seL4 protects
strongly — within the scope stated above.

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
| OneCLI Agent Vault | **Keystore PD**: extends `rpi4-tpm-pd`'s broker pattern; holds API keys sealed to PCRs and acts as a *policy-enforcing credential-use service* (see below), not a mere header injector | TPM broker exists (`rpi4-tpm-pd/src/main.rs`) |
| Channel adapters (WhatsApp, …) | **Channel PDs** behind the https PD | new |
| Memory (CLAUDE.md, notes) | **Storage PD** owning the SD card | new (no storage driver yet) |
| Scheduled jobs | **Timer PD** (generic timer, `CNTVCT_EL0`) | new (also needed by smoltcp — see networking roadmap Phase 4) |
| Claude Agent SDK loop | **Agent-core PD**: conversation state machine, tool dispatch | new |
| Host bash / tools | **Tool slot PDs**: pre-declared generic PDs the supervisor loads code into | new |

The key architectural rule carried over from nanoclaw, but enforced by
the kernel instead of by convention: **the agent-core PD composes API
requests but holds no credentials and no generic authenticated-HTTPS
authority**. A fully prompt-injected agent-core PD cannot exfiltrate
the key, read another PD's memory, or touch a device it wasn't mapped.

### The keystore is a policy engine, not a header stamper

Review-driven correction: hiding the key bytes prevents **extraction**,
not **abuse**. A compromised agent-core with access to an unrestricted
credential-injection oracle doesn't need the literal key — it can spend
without limit, exfiltrate anything it can read through *authorized* API
calls, and use the API as a covert channel. So the keystore PD is
specified as a **credential-use service**: agent-core presents a
request together with an **attenuated request capability** the keystore
previously granted, and the keystore enforces, per request:

1. permitted destination (pinned `api.anthropic.com` endpoints only),
2. permitted operation (e.g. `POST /v1/messages`, nothing else),
3. maximum request size,
4. rate and cumulative-cost limits (token budget as a hard ceiling),
5. model allowlist,
6. data-classification restrictions (e.g. entries tagged
   local-only by the provenance labels in the control-plane section
   never leave the device), and
7. an owner-auditable log entry per credential use.

This policy check is a small deterministic state machine over typed
requests — precisely the shape Verus handles well, and a far more
valuable proof target than any amount of model arithmetic.

**Who owns TLS (explicit decision).** Bearer-token APIs leave three
options: keystore owns TLS (sees all plaintext), https PD owns TLS and
receives reusable credentials (defeats the point), or split-TLS
exotica. This design picks the first, with eyes open: **the keystore
PD owns the TLS client, certificate pinning, and session keys for
credentialed endpoints**, and therefore sees prompts and responses for
those sessions. That makes it the privacy-sensitive heart of the TCB —
which is accepted and managed: it is the smallest PD in the system, it
is the highest-priority verification target, and DNS resolution for it
is pinned/static so no other PD can redirect it. The separate https PD
in the diagram handles *uncredentialed* traffic only (update capsules,
public fetches), where TLS termination outside the keystore leaks
nothing.

## The agent control plane: authority flow, not just isolation

Isolation answers "what memory can a compromised component touch?" The
central agent-security question is different: **which authority may
flow from untrusted text to consequential action?** If agent-core
legitimately holds a channel to a tool broker, an injected prompt
invokes that legitimate authority — capability security does not solve
this by itself. The design therefore adopts a **split-agent control
plane**:

- **The LLM (local or cloud) never directly controls tools.** It emits
  a *typed proposed action* — a structured request, not a tool call.
- **A small deterministic policy engine** (its own PD, or fused with
  the keystore's policy machine) decides whether a proposed action may
  execute, based on: provenance labels on every input (who said this —
  owner keyboard vs. WhatsApp stranger vs. fetched web content),
  information-flow labels on data the action would touch, per-action
  authorization policy, and explicit declassification rules.
- **High-impact actions require human confirmation** on the trusted
  I/O path — the input and graphics PDs, which no other PD can forge
  or overdraw, are exactly the trusted-display/trusted-input primitive
  this needs. That's a genuinely nice consequence of the existing
  architecture: the confirmation dialog is rendered by a PD the agent
  cannot touch.
- **Non-bypassability is a capability-topology fact, not a promise:**
  agent-core's `.system` entry has channels to the policy PD and
  nothing else consequential — a property the topology checker
  (verification section) machine-checks on every commit.

The policy engine's state machine — authority delegation, approval
flows, budget accounting, declassification — is the single most
valuable Verus target in the system, ahead of anything in the
inference path. Verifying *this* is what makes the appliance an
"agent" rather than a very secure pipe to an unconstrained model.

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

For agent/tool slot PDs, updates without reboot. Structural decision
first, per review: **"the supervisor" is not one PD.** A single PD that
verifies updates, writes executable memory, controls restarts, owns
epochs, and coordinates measurement is an ambient authority whose
compromise defeats every property at once. The role decomposes into:

- **lifecycle PD** — the Microkit parent: fault handling, stop/restart,
  epochs; touches no update bytes;
- **update-verifier PD** — parses capsules, checks signatures and
  anti-rollback; its only output is a **one-shot install authorization**
  (a signed token naming blob digest + target slot + generation) sent
  to the installer;
- **installer PD** — tiny, holds the `rw` mapping to slot code regions,
  and writes *only* what a fresh one-shot authorization names;
- **measurement/event-log** stays in the keystore/TPM broker;
  **rollback state** lives in TPM NV, owned by the verifier.

"Verified artifact may execute once in slot N" becomes an explicit,
consumable object rather than a standing power.

The install lifecycle (normative order — each step's omission is a
concrete attack):

1. Slot PDs declare their executable region as an explicit
   `memory_region` mapped `rx` into the slot and `rw` into the
   installer only (Microkit can't hand a parent the child's *original*
   program image, so the slot's payload lives in this region).
2. A capsule arrives via the https PD or storage. Beyond
   `{blob, version, signature}`, the signed header **binds** target
   platform, slot and PD type, load address and entry point, ABI/
   protocol version, dependency and config hashes, rollback epoch, and
   expiry — otherwise a validly signed artifact can be replayed into
   the wrong slot or under an incompatible interface (workplan IC-2 is
   the normative encoding).
3. Verifier PD: totality-parse header → verify signature over
   header+payload from a **private staging buffer** (no other PD maps
   it — no TOCTOU) → check monotonic version against TPM NV → emit
   one-shot authorization; keystore extends the PCR (the event-log
   machinery in `rpi4-tpm-boot/src/boot_chain.rs`/`attestation.rs`).
4. Lifecycle PD: `microkit_pd_stop(slot)`. Installer: copy staged blob
   to the slot region, **clean D-cache and invalidate I-cache** for the
   range (skipping this is a real correctness/security bug on ARM),
   validate the entry point against the capsule header. Lifecycle PD:
   zero ring indices, bump epochs, `microkit_pd_restart(slot, entry)`.

Honest limitation: Microkit's static mappings mean the installer's
`rw` alias **cannot be revoked at runtime** — per-address-space W⊕X
holds, but a live writer to executable memory permanently exists.
The design compensates by making the installer minimal, verified, and
inert without a fresh authorization; true revocation (unmap between
updates) requires dropping below Microkit to dynamic seL4, noted as
future work. Blobs are position-independent or linked to the slot's
fixed base; slot PDs get a deliberately minimal capability set (rings
to agent-core, nothing else), which is what makes running
freshly-downloaded code in them acceptable.

What attestation then gives a remote party — stated precisely: a TPM
quote establishes the **measured boot + update history**, *assuming*
the verifier trusts the root of trust, the measurement agent, the
event-log reconstruction, the mapping from digests to approved
artifacts, and the absence of unmeasured execution paths. It does not
by itself prove anything about what the code *did* — per-response
claims need the execution receipts described in the local-inference
section.

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

### Assurance case

Claim by claim, with mechanism and honest status — this table is the
document's real thesis:

| Claim | Mechanism | Proof / validation status |
|---|---|---|
| PD memory isolation | seL4 capabilities | inherited **only** for the verified configuration/architecture; see scope + DMA caveats |
| Capability topology matches docs | `.system` checker in CI | proposed |
| Ring safety incl. restart | Verus | input ring: verified today; epoch extension: proposed |
| Update authorization (verify-before-write, measure-before-execute, anti-rollback) | Verus on verifier/installer | proposed |
| Update crash-atomicity | TLA+ model | proposed |
| Model parser / memory envelope safety | Verus | proposed |
| Model/code identity (what's installed) | measurement + TPM quote | proposed; conditional on RoT/event-log trust |
| Per-response inference provenance | signed execution receipts + deterministic re-execution | proposed; receipts are *challengeable claims*, not proofs |
| Credential-use policy compliance | keystore policy state machine (Verus) | proposed |
| Agent policy compliance (authority flow) | control-plane policy engine (Verus) + human confirmation | **absent** — hardest, most valuable open item |
| Safe tool behavior | capability mediation + per-tool reasoning | **absent**; mediation bounds blast radius only |
| Timing/covert channels, physical attacks | — | out of scope, stated |

The composed *conditional* claim: if seL4's proofs hold for our
configuration, the topology checker passes, the DMA caveat is respected
(no claim of DMA-immunity on Pi-class hardware), and the
verifier/installer/keystore/ring proofs discharge, then a fully
compromised agent, tool, or channel PD cannot read credentials, corrupt
another PD, persist across a signed update, or evade the measured
update history. What remains unverified and tested-only: smoltcp, the
TLS implementation (mitigated by verified crypto underneath and cert
pinning), device drivers (mitigated by capability scoping, *except* the
DMA hole), and the LLM's behavior itself — which is mitigated by the
control-plane policy engine, not by any property of the model.

## Threat model

Explicit attacker classes and what the design does — and does not —
provide against each:

| Attacker | Containment | Residual risk |
|---|---|---|
| Compromised agent-core (prompt injection, bug) | no credentials, no devices, typed-action mediation, keystore policy limits, human confirmation for high impact | covert signaling within policy limits; garbage within budget |
| Malicious model weights | verified loader (parse safety), capability-empty model PD, measured identity | model outputs bad *content* — handled by control plane, not isolation |
| Malicious tool capsule | signature required, slot PD minimal caps, restartable | signed-but-buggy tools act within their capability set |
| Malicious channel input (WhatsApp/email/web) | channel PD isolation + provenance labels + policy engine | social engineering of the *owner* at confirmation prompts |
| Compromised network peer / MITM | TLS + cert pinning in keystore; signed capsules end-to-end | DoS/blocking (availability) |
| Compromised verifier/installer/lifecycle PD | decomposed roles, one-shot authorizations, proofs targeted here first | a *proven-wrong* proof; residual ambient authority in installer's rw alias |
| DMA-capable device / driver PD (Pi-class) | fixed carve-outs, minimal Rust drivers | **system-wide memory access — unmitigated on BCM2711**; choose platform accordingly |
| Physical attacker | measured boot (detect), OTP signed boot on CM4 (prevent boot of tampered images), sealed keys | hardware implants, closed-ROM trust, chip-level attacks: out of scope |
| Resource exhaustion | fixed arenas, ring capacities, budget ceilings, watchdog restarts | sustained flooding degrades service (availability is the weakest proof) |

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

Can the device run its own LLM — llama.cpp-style — inside a PD? Yes.
Two routes; **Route B is the novel one and the one this project should
lead with**, because it's where "formal verification experiments" and
"local inference" actually intersect rather than merely coexist.

### Route B: a verified-substrate native inference PD (novel — lead)

A llama2.c-scale engine — a few hundred lines of transformer inference
— ports cleanly to a no_std Rust PD: no libc, no threads required, and
**pre-allocated arenas instead of malloc**, which is exactly the
allocation discipline `docs/decoder-allocation-security.md` already
prescribes for the image decoders. Weights load from a big Microkit
memory region; quantized matmuls use NEON. Realistic for
sub-1.5B-parameter models; forget 7B.

What makes this genuinely new rather than just small: **nobody ships an
inference stack whose loader, memory discipline, and isolation
substrate are all verified.** llama.cpp's own history shows where such
a stack actually breaks — not in the math, but in parsing and
allocation (malformed-GGUF heap overflows are real, published CVEs;
"model file" is just "malicious media file" wearing a lab coat). Those
are precisely the classes this repo's toolchain eliminates:

- **Verified model loader.** GGUF parsing as a Verus totality target:
  every header/tensor-offset/quantization-block read proven in-bounds,
  every malformed file cleanly rejected, no integer overflow in size
  arithmetic. This is the same proof shape as the ring buffers, applied
  to the scariest new input format of the decade.
- **Verified memory envelope.** All inference buffers (KV cache,
  activations, scratch) carved from fixed arenas sized at load time
  from *verified* header fields — provable absence of allocation-based
  DoS, extending the decoder-allocation-security argument.
- **Panic-freedom of the whole PD** — an availability property the
  supervisor's restart tier then backstops.
- **Deterministic inference → re-execution verifiability (stated
  narrowly).** No threads, no malloc, fixed arenas, integer/quantized
  kernels make bit-reproducibility *achievable* — conditional on
  pinning the tokenizer version, prompt serialization, quantization
  semantics, integer-overflow behavior, compiler/SIMD codegen, sampling
  algorithm, RNG + seed, context-truncation rules, and model config.
  That's why the workplan makes determinism a *CI-tested property*
  (asserted output hash), not an assumption. What determinism buys is
  **re-execution verification**: a verifier holding the same measured
  weights can replay (input, seed) and check the output — the
  challenge/re-execute pattern of
  [EigenAI](https://arxiv.org/abs/2602.00182). It does *not* make a
  TPM quote a proof of inference.
- **Signed execution receipts** are the per-response artifact: the
  model PD emits, and a device-held attestation key signs,
  `{nonce/session, input digest, weights + runtime measurements,
  config + sampling params, output digest}`. A receipt
  cryptographically *binds* an answer to a measured model and input —
  making the claim challengeable by re-execution — but attested
  execution still doesn't establish the model behaved well
  (the limits discussed for attested guardrails in
  [Proof-of-Guardrail](https://arxiv.org/abs/2603.05786) apply
  verbatim).
- The hot loops (matmul/attention) stay ordinary unverified Rust with
  NEON — Verus proves the *envelope* (bounds, sizes, totality), not
  the linear algebra; property tests against a reference
  implementation cover numerics. Honest division of labor, same as
  the crypto rule.

It also starts *earlier* than Route A: a stories15M-class model with
embedded weights needs only rings + a PD — demoable in the existing
QEMU CI today, no libvmm dependency, then scale to real weights via
the storage PD and signed capsules.

### Route A: a VM PD running Linux + llama.cpp (pragmatic bridge)

Noted as the fallback for when someone wants an off-the-shelf model at
full llama.cpp maturity. It wants libc, libstdc++, pthreads, and mmap,
so instead of porting: **run it in a virtual machine that is itself a
child of a PD.** Microkit supports this natively — `<virtual_machine>`
elements with vCPUs, where the parent PD is the VMM and receives all
guest faults
([manual](https://github.com/seL4/microkit/blob/main/docs/manual.md)),
with [libvmm](https://github.com/au-ts/libvmm) as the AArch64 VMM
library (in development; boots Linux guests; the LionsOS pattern).
The guest Linux sits inside the model PD's capability box, not the
system's TCB — no device mappings, no credentials, rings to agent-core
only — and the `cpu` attribute pins it to cores 2–3 away from the
interactive PDs. Bring-up on `qemu_virt_aarch64` first; RPi4
hypervisor-mode support (EL2 + GIC-400 quirks) needs validation. The
trade: you inherit a Linux kernel and all of llama.cpp inside the box,
so the *loader* guarantees of Route B don't apply — isolation is the
only story, which is nanoclaw's story. Route B is the one that says
something new.

### What the security architecture buys you here

1. **Attested model identity + challengeable response provenance.**
   Weights ship as Tier-2 signed capsules, measured into a PCR like any
   code blob — that attests *which model is installed*. Execution
   receipts (above) then bind individual answers to that measured
   model, verifiable by re-execution. Neither mainstream local
   inference nor cloud APIs offer this combination; stated this way it
   is defensible, where "prove which model produced an answer" via
   quote alone was not.
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

Sequencing: the *full* model PD (real weights, signed capsules,
routing) is post-Phase-C, but Route B's core — verified GGUF loader +
arena engine + tiny embedded model in a QEMU CI test — has no
dependency on the supervisor or networking and can start immediately as
its own track. It's also the strongest standalone artifact this repo
could publish. Route A waits on libvmm bring-up and is optional.

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

- **Phase 0 — the flagship artifact (leads everything, per review):**
  tiny deterministic inference PD with a verified bounded model loader
  and **signed execution receipts** (workplan WP-6). Self-contained,
  more novel, and easier to evaluate than the Claude-connected
  appliance; publishable on its own.
- **Phase 0.5 — substrate decision:** timeboxed evaluation of adopting
  LionsOS/sDDF components for network/timer/storage vs. bespoke
  (workplan WP-0); decides how much of Phase A below is *ours to
  build*.
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

## Related work

No existing project combines a verified microkernel, a personal
tool-using agent, local inference with a verified loader/envelope, and
attested output provenance — but every piece has adjacent prior art
that this design should acknowledge and, in two cases, possibly adopt:

- **KataOS / Sparrow (Google):** the closest conceptual predecessor —
  an seL4 + Rust + component-isolation platform aimed at ambient ML
  devices, with hardware-rooted identity. "Secure seL4 appliance
  hosting ML" is not a new category; the claimed novelty here is
  narrower and must be stated as such: the verified inference
  *boundary* and the agent *authority architecture*.
- **[LionsOS](https://arxiv.org/abs/2501.06234) + sDDF (Trustworthy
  Systems):** a Microkit-based modular OS with exactly the driver/
  service framework this design would otherwise rebuild bespoke.
  **Open substrate decision (workplan WP-0):** build agent-specific
  PDs on LionsOS services vs. continue this repo's own plumbing. The
  honest default is to adopt where components fit (network, timer,
  storage) — spending the project's novelty budget on the agent
  control plane and inference boundary, not on commodity OS plumbing.
- **Project Veracruz:** attestable computation as {measured runtime,
  policy document, approved principals, attested session} — and the
  inspiration for a serious alternative to native tool capsules:
  **tools as WebAssembly modules** in a Wasm-runtime PD, with explicit
  host imports, bounded memory, fuel limits, and typed capability
  handles. A narrower loader and a more analyzable update surface than
  arbitrary ELF blobs, at the cost of trusting the Wasm runtime.
  Worth a spike (workplan WP-18) before committing to native slots
  for *tools* (system PDs stay native).
- **Project Oak / confidential-computing runtimes (CoCo, Gramine,
  Enarx):** measured workloads + remote attestation on TEEs — larger
  TCB, weaker host-runtime assurance, but far better hardware and
  accelerator support. This is the pragmatic baseline for the cloud
  route, and the honest comparison point for "why seL4 at all."
- **[VECODI](https://arxiv.org/abs/2606.07470):** verifiable/
  confidential DNN inference on constrained devices via a *minimal
  trusted monitor* around **untrusted optimized inference code** —
  an alternative decomposition to Route B worth keeping in mind if
  making the whole native engine panic-free proves expensive: verify
  and attest model identity, memory ownership, sequencing, and output
  commitment; leave the kernels untrusted.
- **[EigenAI](https://arxiv.org/abs/2602.00182)** (deterministic
  inference + challenge/re-execution) and
  **[Proof-of-Guardrail](https://arxiv.org/abs/2603.05786)** (TEE-attested
  guardrail execution, and its explicit limits) — the two results that
  calibrate this design's receipt and attestation claims, cited in the
  local-inference section.

## Bottom line

Nothing in the nanoclaw model is out of reach; the repo has been
unknowingly building its substrate. The demos established the isolation
grammar (PDs + verified rings + broker PDs + measured boot); the agent
appliance is that grammar applied to a new vocabulary: lifecycle/
verifier/installer, keystore-as-policy-engine, control plane,
agent-core, slots. The genuinely new kernel-adjacent work is small and
well-supported by the pinned SDK (hierarchical PDs); the long-pole
engineering is unglamorous (TCP, TLS, storage, update plumbing — much
of it possibly adoptable from LionsOS) and CI-provable in QEMU before
ever touching the Pi.

The differentiated contribution, stated at its defensible size:
**seL4-enforced authority boundaries + a formally verified
action/update control plane + verified model-file and memory-envelope
handling + cryptographically bound deterministic inference receipts.**
That combination is ahead of every adjacent system in Related work —
and it is a precise claim, not "a verified agent OS."
