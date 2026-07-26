# Attestable Builds on build.confidential.ai: Evaluation

**Status: evaluation / adoption plan — no integration is implemented yet.**
Repo facts below are cited against files in this repository; claims about
the build.confidential.ai service come from its public papers and
open-source implementation and are marked as unverified where we could
not confirm them directly (the service itself was not reachable from the
environment this evaluation was written in).

Question under evaluation: **can we build our seL4-based OS as an
attestable build on build.confidential.ai?**

## Verdict

**Yes — but conditional on packaging our build for a toolchain Kettle
supports**, and with one important scoping caveat.

Kettle drives a *build system* (Cargo, pnpm, or Nix), not an arbitrary
container recipe. Our top-level build is GNU Make orchestrating cargo
and then the SDK's `microkit` tool, which is none of those, so the work
Phase 1 actually requires is a Nix derivation wrapping that Make build
(see the adoption path). Nothing about the build makes this hard — it is
packaging work, not a blocker — but it is real work that does not exist
yet. In the meantime `kettle build` runs outside a TEE, so unsigned SLSA
provenance is available immediately as a stepping stone.

The current build consumes the prebuilt Microkit SDK 2.1.0 (pinned by
SHA-256 in `build-system/config/versions.mk`) rather than compiling the
seL4 kernel from source. So a Phase-1 attestation would say, precisely:

> *"This `loader.img` was produced inside a hardware-measured
> confidential VM, from commit X of this repository, using exactly these
> hash-pinned inputs: Microkit SDK 2.1.0 (SHA-256
> `faff1b6d…`), Rust `nightly-2026-07-02`, and the committed
> `Cargo.lock` dependency set."*

It would **not** say "the seL4 kernel binary inside that image was
compiled from audited seL4 sources" — kernel provenance bottoms out at
the seL4 project's published SDK release binary until Phase 2 (an
attested from-source SDK build, see the adoption path below). That is an
honest and still-useful claim: it eliminates the build machine, the CI
operator, and the artifact-hosting path from the trust story, leaving
the SDK binary as the one externally-trusted input — and that input is
at least hash-pinned and publicly published by seL4.

Setting that packaging step aside, the build suits the service well: it
is an x86 Linux cross-compile with no target hardware in the loop, and
every external input is pinned by hash. The other gaps found during this
evaluation are small and most were fixed alongside this document.

## What build.confidential.ai is

build.confidential.ai is "Kettle", Confidential AI's attestable build
service. It commercializes the *Attestable Builds* research line
(Hugenroth et al., CCS 2025 — <https://arxiv.org/abs/2505.02521>,
<https://dl.acm.org/doi/10.1145/3719027.3765128>) and has a companion
paper, *Kettle: Attested Builds for Verifiable Software Provenance*
(Arko & Asad — <https://arxiv.org/abs/2605.08363>), plus an open-source
implementation at <https://github.com/lunal-dev/kettle> (installer and
release artifacts are published under
<https://github.com/confidential-dot-ai/kettle> — same project, the org
was renamed).

The mechanism, per those sources:

- Builds run inside a **measured confidential VM** (AMD SEV-SNP or
  Intel TDX; the CCS paper also targets AWS Nitro Enclaves). The host
  and the service operator are outside the trust boundary; only the TEE
  hardware is trusted.
- A build records the source commit, dependency set, toolchain, build
  environment, and output artifact digests into a `provenance.json`
  produced *inside* the CVM. The SHA-256 of that document is committed
  into the report-data field of the TEE attestation report, so the
  hardware-signed attestation report itself signs the provenance
  (packaged as `evidence.json`).
- Anyone can then run `kettle verify`: it reads `evidence.json`, checks
  the signature chain against the hardware vendor's public keys
  (AMD/Intel), validates `provenance.json`, and confirms the artifact
  checksum.
- The service claims SLSA Build L3, and describes hardening of the
  build environment itself ("MAC policies, seccomp filters, and process
  isolation keep the build environment unchanged between source
  checkout and artifact emission" — vendor wording).
- GitHub integration: connect a repository, and every commit is checked
  out and built inside the TEE, producing signed provenance.
- Overheads reported in the CCS paper's prototype: ~42 s startup
  latency and ~14 % build-time overhead; it built complex projects
  (e.g. LLVM/Clang) unmodified.

### The `kettle` CLI

The open-source client is installable from a release installer script,
`cargo install --git`, or a prebuilt "reproducible" binary; building it
with attestation support needs the `attest` feature and `libtss2-dev`.
Three commands matter:

- **`kettle build`** — generates SLSA provenance, runs the build, and
  checksums the outputs. Runs *outside* a TEE, which makes it usable in
  ordinary CI today for everything except the hardware signature.
- **`kettle attest`** — runs the build inside the TEE, records the VM
  firmware and OS image, and hardware-signs the result.
- **`kettle verify <directory>`** — validates binaries against hardware
  vendor public keys. `--igvm <FILE>` additionally checks the launch
  measurement against an IGVM file's launch digest; `--image <FILE>`
  validates a dm-verity roothash and requires `--igvm`.

Outputs land in a `kettle-build/` directory: `provenance.json` (an
in-toto Statement carrying the SLSA Provenance v1.2 predicate),
`evidence.json` (the hardware-signed attestation), and checksums. A
multi-artifact build is handled by that directory rather than by any
per-artifact configuration.

### Supported toolchains — the binding constraint for us

Kettle has **no build-configuration file format**. It drives a build
system it knows how to run, and today that list is:

| Toolchain | Status |
|---|---|
| Cargo (Rust) | supported |
| pnpm (JS/TS) | supported |
| Nix (any language) | supported |
| uv (Python), Go | planned |

Build types are toolchain-specific by design, so verifiers can apply
build-type-specific policy instead of parsing free-form fields. The
practical consequence for this repo is in the next section.

### Attestable vs. reproducible builds

This is the key conceptual point. Classic reproducible builds prove
provenance by *bit-for-bit determinism*: anyone can rebuild and compare
hashes, but getting a real toolchain to be deterministic (timestamps,
path embedding, parallelism, codegen ordering) is a long grind, and
someone still has to actually do the independent rebuilds.

Attestable builds substitute a **hardware root of trust for
determinism**: the build does *not* need to be reproducible ("no
deterministic compilers required"), because the TEE attests that a
specific, measured build environment transformed specific, hashed
inputs into a specific, hashed output. The verifier trusts AMD/Intel's
attestation keys and the measured build image instead of trusting
rebuild-and-compare.

The known structural limitation (stated in the CCS paper) is the
**bootstrap problem**: the verifier must trust the expected launch
measurement of the build image itself. The paper's recommendation is
that this first image be produced via reproducible builds, pushing
determinism down to one small, slow-changing artifact rather than every
project build.

## How this repo's build maps onto Kettle

Facts below are verifiable in-tree. Two of the three points are a clean
fit; the third is the packaging work Phase 1 turns on.

**No target hardware at build time.** The build cross-compiles from
x86-64 Linux to AArch64 (and RISC-V): `qemu-e2e.Containerfile` installs
`gcc-aarch64-linux-gnu` and `gcc-riscv64-linux-gnu` on `ubuntu:24.04`,
and the custom Rust target specs live in
`build-system/targets/aarch64-sel4-microkit.json` and
`riscv64gc-sel4-microkit.json`. Kettle's CVMs are x86 Linux machines;
nothing in this build needs a Raspberry Pi, a TPM, or any peripheral.

**Hash-pinned inputs.** `build-system/config/versions.mk` pins:

```make
MICROKIT_VERSION := 2.1.0
MICROKIT_SDK_SHA256 := faff1b6d6b546cbb0bfea134588499533130d406ae2a5e533e791ddf23ac7599

RPI_FIRMWARE_TAG := 1.20250915
UBOOT_VERSION := v2025.10
```

The Rust toolchain is pinned to `nightly-2026-07-02` in
`rust-toolchain.toml` (with `rust-src` for the
`-Z build-std=core,alloc,compiler_builtins` builds configured in
`build-system/config/defaults.mk`). `release.yml` verifies the SDK
tarball against the same SHA-256 before use.

**But the build is Make-driven, which Kettle does not run.**
`make PRODUCT=<p> PLATFORM=<plat>` in `build-system/` builds the Rust
protection domains via cargo and then has the SDK's `microkit` tool link
them into `loader.img` (the `SYSTEM_IMAGE` in each
`build-system/config/products/*.mk`). GNU Make is not one of Kettle's
runners, so the build cannot be handed to the service as-is.

`qemu-e2e.Containerfile` — which takes the toolchain version and SDK
hash as `ARG`s and verifies the SDK download with `sha256sum -c` — is a
faithful description of the build environment, but it is *not* a
substitute for a supported toolchain: Kettle consumes a build system,
not a container recipe.

Two ways out, only one of which covers the shipped artifact:

- **Nix derivation (the real path).** Wrap the existing Make build in a
  derivation. Kettle's Nix runner is language-agnostic, so this covers
  the whole pipeline through `loader.img` and the SD-card image. The
  hash-pinned inputs already in `versions.mk` map onto Nix's model
  naturally.
- **Cargo runner alone (insufficient).** Kettle could build the
  protection-domain crates directly, but the `microkit` link step that
  produces the artifact we actually ship would sit outside the
  attestation. That attests components, not the image.

**No privileged operations required.** Kettle's build CVM presumably
restricts privileged syscalls; helpfully, our SD-card image assembly
(`build-system/include/sdcard.mk`) uses **mtools and requires no root
and no loop devices** — the comment in that file says so explicitly and
only the *write-to-physical-card* target uses `sudo dd`, which is a
developer-machine operation, not a build step. So even full SD-card
image artifacts should be buildable inside the CVM.

### What the attestation would and would not claim

Would claim:

- The artifact digest of `loader.img` (and, if we choose, the SD-card
  image and `SHA256SUMS.txt`) was produced from a named commit of this
  repository.
- The exact toolchain and dependency set used, including the Microkit
  SDK tarball hash, the pinned nightly, and every crate in
  `Cargo.lock`.
- The build environment was a measured CVM whose operator could not
  tamper with the build.

Would not claim:

- That the seL4 kernel inside the SDK was compiled from any particular
  source. Until Phase 2, kernel provenance is "the binary seL4
  published for Microkit 2.1.0, hash-pinned".
- Anything about the *verification status* of that kernel. As
  `docs/secure-agent-os.md` documents in detail, Microkit deploys the
  seL4 MCS kernel, whose code-conformance proofs for our configuration
  are still in progress; build attestation and formal verification are
  orthogonal claims and neither substitutes for the other.
- Anything about runtime behavior on the device — that is the job of
  the TPM measured-boot work (`rpi4-tpm-boot/`), which Phase 3 connects
  to this.

## Gap analysis

| Gap | Impact on attestation | Status |
|---|---|---|
| `Cargo.lock` was gitignored (root `.gitignore`) for the seL4 crates, so the crate dependency set was resolved at build time, not commit time | Provenance would cover a dependency set the repo does not actually pin | **Fixed alongside this evaluation** (lockfiles committed for every workspace root; the `lockfile-check` CI job fails if any drifts from its manifest) |
| `qemu-e2e.Containerfile` base was the mutable tag `ubuntu:24.04` | Build-image measurement drifts as the tag moves; weakens "same environment" claims | **Fixed alongside this evaluation** (digest pin, exercised by the `container-image.yml` workflow incl. a weekly drift tripwire) |
| RPi firmware checksums in `rpi4-graphics/checksums.sha256` were placeholders ("need to be populated after first download") while `start4.elf`/`fixup4.dat`/DTB go into shipped SD-card images | Unpinned binary inputs inside an attested artifact | **Fixed alongside this evaluation** (hashes populated; firmware downloads in `rpi4-graphics/Makefile` and `build-system/include/sdcard.mk` now fail on mismatch, and `scripts/download-microkit-sdk.sh` verifies the SDK tarball) |
| Version pins duplicated across `versions.mk`, `rust-toolchain.toml`, `qemu-e2e.Containerfile` `ARG`s, and several `.github/workflows/*` files (e.g. the SDK hash is repeated verbatim in `release.yml`) | A skewed update could attest a build that doesn't match developer builds | **Enforced by CI** — `scripts/check-pins.sh` (the `pin-consistency` job) treats `versions.mk` + `rust-toolchain.toml` as sources of truth and fails on any divergent copy; physically centralizing the pins remains optional cleanup |
| No provenance, artifact signing, or SBOM anywhere today — `release.yml` emits only `SHA256SUMS.txt` computed on the (untrusted) GitHub runner | This is precisely the hole the service fills; nothing to fix in-repo beyond adopting it | **Addressed by adoption itself** |
| Top-level build is GNU Make (driving cargo, then the `microkit` tool), which is not one of Kettle's runners (Cargo/pnpm/Nix) | Blocks handing the build to the service as-is; the Cargo runner alone would leave the `microkit` link step — and therefore the shipped image — outside the attestation | **Open — the main Phase 1 work item**: wrap the Make build in a Nix derivation |
| Kernel is a prebuilt SDK binary, not built from source | Attestation chain bottoms out at seL4's release artifact | **Phase 2** (below) |

## Open questions about the service

These could not be verified from this environment and should be
answered before committing (the site was unreachable; the papers and
the open-source repo do not settle them):

1. **Pricing and access model** — self-serve vs. contact-sales; open
   -source project terms.
2. **Resource and duration limits** of the build CVM. Our full builds
   (`-Z build-std` plus multiple protection domains) are modest by
   LLVM standards, so the paper's results suggest this is fine, but
   limits should be confirmed.
3. **Privileged operations policy** inside the CVM. Likely moot for us
   (mtools-based image assembly needs none), but worth confirming for
   any future step that might want loop devices or FUSE.
4. **Nix runner specifics** — how a derivation is invoked, whether
   network-fetching derivations (our SDK and firmware downloads) need
   to be fixed-output, and how flake inputs surface in
   `provenance.json`. This is the one that decides how much of the
   Phase-1 packaging work is mechanical.

Two earlier questions are now answered and dropped: there is no build
configuration format (Kettle drives a supported toolchain), and
multi-artifact builds are handled by the `kettle-build/` output
directory rather than per-artifact config.

If the hosted service does not fit on any of these, the fallback is
**self-hosting the open-source Kettle** on a SEV-SNP or TDX cloud CVM —
same verification story, more operational burden (see Alternatives).

## Phased adoption path

### Phase 1 — attest the current SDK-based build

Two steps, the first of which needs nothing from the service.

**1a. Unsigned provenance in CI now.** `kettle build` runs outside a
TEE, so a CI job can emit `provenance.json` and checksums for our
existing artifacts immediately. That puts the provenance schema, the
artifact list, and the verification habit in place while the packaging
work below happens. It proves nothing about the build machine — GitHub
runners are still trusted at this stage — but it is a strictly better
release record than today's bare `SHA256SUMS.txt`.

**1b. A Nix derivation, then `kettle attest`.** Package the Make build
(`make PRODUCT=… PLATFORM=…` per product, mirroring what
`scripts/build-microkit.sh` does in `ci.yml`'s `microkit-build` job and
`release.yml`'s `build-microkit` job today) as a Nix derivation, so
Kettle's language-agnostic runner can drive the whole pipeline through
`loader.img`. The pinned inputs in `build-system/config/versions.mk` —
SDK tarball hash, firmware hashes, U-Boot version — map onto
fixed-output derivations, which fits what the pin-consistency check
already enforces. Then connect the repository (or self-host Kettle on a
SEV-SNP/TDX CVM) and run `kettle attest`, producing `evidence.json`
alongside each release's `loader.img` and SD-card image, verifiable by
anyone with `kettle verify`.

The remaining prerequisites — committed lockfiles, a digest-pinned
build environment, hash-pinned firmware — are done and CI-enforced.

### Phase 2 — attested from-source build of the Microkit SDK

Build the SDK itself — the seL4 kernel and the `microkit` tool — from
the upstream `seL4/microkit` sources inside an attested build, pin
*that* attested SDK artifact in `versions.mk`, and have Phase-1 builds
consume it. Then the provenance chain runs source-to-image with no
opaque binary in the middle. This is especially worthwhile for this
project: the value of seL4's l4v proof effort is a claim about the
*source*; an attested from-source build is what entitles us to claim it
about the *binary we ship* (subject to the MCS proof-status caveats in
`docs/secure-agent-os.md`).

### Phase 3 — close the loop with device attestation

Feed attested artifact digests into the runtime trust machinery that
already exists in this repo:

- **Update capsules** (`update-capsule/`): mint ed25519-signed capsules
  (Verus-verified header parser, `src/header.rs`) only over
  Kettle-attested payload digests, so a device accepting an update is
  transitively accepting a build with hardware-signed provenance.
- **TPM measured boot** (`rpi4-tpm-boot/`): derive expected PCR values
  from the attested image digests, so a remote verifier checking a TPM
  quote is checking against measurements that chain back to
  `evidence.json`, not to a trusted release engineer.
- **Inference receipts** (`rpi4-llm/src/receipt.rs`): a signed receipt
  from a device whose boot measurements chain to an attested build
  gives an end-to-end story — *this output came from this model on this
  software, and here is the hardware-rooted evidence for every link*.

## Alternatives considered

**Classic reproducible builds.** Stronger in one way (no trust in
AMD/Intel attestation infrastructure) but much more work: bit-for-bit
determinism across rustc nightly, `build-std`, and the SDK's `microkit`
tool would need auditing and ongoing maintenance, plus an actual
community of independent rebuilders to be meaningful. Worth pursuing
eventually for the small Phase-2 bootstrap image (which is also the
paper's own recommendation for Kettle's build image), not as the
primary mechanism.

**SLSA provenance on GitHub Actions.** Cheap to add to `release.yml`
and better than nothing, but the attestation root is the GitHub-hosted
runner and GitHub's signing infrastructure — the operator we are trying
to remove from the trust story remains inside it. Reasonable interim
step; strictly weaker claim. Note that Phase 1a (`kettle build` in CI)
has the same trust root but emits the same provenance schema we would
later hardware-sign, so it is the better-aligned interim move.

**Self-hosted Kettle.** Same verification semantics as the hosted
service (the verifier checks hardware signatures, not the operator),
at the cost of operating SEV-SNP/TDX CVMs ourselves. The right fallback
if hosted pricing or CVM limits (open questions above) don't fit; also a
reasonable end-state for Phase 2's SDK-bootstrap build where we may want
full control. Either way the Nix packaging in Phase 1b is the same
prerequisite.
