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

## Honest platform caveats

- **RPi4 has no real secure boot.** The boot ROM and VideoCore firmware
  are closed and can't be made to verify our chain, so a physical
  attacker can replace the image. The TPM gives *measured* boot —
  tampering is detectable via attestation and secrets sealed to PCRs
  become unrecoverable — but not *verified* boot. The repo already
  acknowledges the VideoCore trust gap (`rpi4-spi-display/README.md`).
  Acceptable for a personal device; document it, don't oversell it.
- **The LLM is remote.** Agent quality/behavior is Anthropic's model +
  our prompts; the OS guarantees are about *containment of effects*,
  credentials, and code integrity — which is the right division of
  labor.
- **63-PD limit** bounds the number of slots; fine for a personal
  agent (a handful of tools + channels), just pre-plan the slot count
  per image.

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
