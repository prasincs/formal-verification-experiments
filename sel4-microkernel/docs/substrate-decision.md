# Wave 1 substrate decision: selective sDDF/LionsOS reuse

Status: **partial adoption; do not migrate the repository substrate in Wave 1**.

This memo resolves WP-0 from `secure-agent-os-workplan.md`. The decision is to
keep the repository's existing Rust/Microkit build and protocol crates as the
Wave 1 substrate, while treating LionsOS/sDDF as the preferred source of
individual C services and design patterns when their interfaces can be wrapped
without changing the pinned toolchain or the fixed authority contracts.

## Evidence snapshot

Evaluation date: 2026-07-03.

| Item | This repository | LionsOS upstream snapshot |
|---|---|---|
| Microkit SDK | 2.1.0, pinned in `build-system/config/versions.mk` | 2.2.0, pinned in `flake.nix` |
| System-description generator | existing hand-authored `.system` files and repository build logic | `au-ts/microkit_sdf_gen` 0.28.1 |
| Primary implementation language | Rust protection domains and Rust protocol crates | C components, Make, Python/SDF generation, Nix development shell |
| Existing verified interfaces | repository-local Verus protocol and parser crates | different component interfaces and generated system descriptions |
| CI runtime validation | QEMU serial-marker jobs | upstream CI states that examples are build-checked; runtime checks are not currently part of that CI |

The evaluated LionsOS commit is
`748ccb4a8cb3c836ab48161e44f9f1e788028520`. The compatibility probe in
`scripts/spikes/lionsos-compat.sh` fetches that exact revision's `flake.nix`
and fails closed unless its Microkit version matches this repository's pinned
SDK.

## Build-spike outcome

A direct build against this repository's Microkit 2.1.0 SDK is **not a valid
upstream-supported configuration** at the evaluated revision. LionsOS's Nix
shell downloads Microkit 2.2.0 and pins `microkit_sdf_gen` 0.28.1. Replacing
only the SDK path would leave the system-description generator and generated
interfaces on the 2.2.0-era contract, so a successful compile would not by
itself establish compatibility.

Reproduce the blocking mismatch:

```sh
cd sel4-microkernel
./scripts/spikes/lionsos-compat.sh
```

Expected result for the evaluated revision:

```text
INCOMPATIBLE: repository Microkit 2.1.0; LionsOS requires 2.2.0
```

The script exits with status 2 for this expected incompatibility. CI runs it in
`--expect-incompatible` mode so upstream drift is visible: if LionsOS changes
to 2.1.0 or removes the pin, the job fails and this decision must be revisited.

## What a netdemo migration would require

The current `netdemo` has a Rust `NetworkDriver` trait, Rust shared-memory
protocol crate, product-specific Make configuration, and serial-marker QEMU
job. A LionsOS/sDDF version would instead require:

1. importing or vendoring the sDDF network, timer, and serial components plus
   their queue libraries;
2. generating a compatible Microkit system description through the upstream
   SDF toolchain;
3. adapting the existing client-facing Rust ring ABI or introducing a checked
   Rust/C boundary;
4. preserving the `.system` authority properties enforced by WP-5;
5. rebuilding the QEMU test around the new component topology; and
6. re-establishing the repository's Verus obligations at the adapter boundary.

That is a substrate migration, not a bounded IP-stack task, and it would couple
WP-1, WP-3, WP-4, WP-5, and WP-11 despite the workplan's parallelization
contract.

## Reuse policy

Wave 1 may reuse LionsOS/sDDF artifacts only under all of these conditions:

- the imported component has a narrow, documented interface;
- its license is retained and compatible with this repository;
- it builds without changing Microkit 2.1.0 or the pinned Rust nightly;
- it does not replace the fixed ring/capsule/TPM contracts;
- the resulting `.system` authority graph remains machine-checked; and
- QEMU runtime acceptance remains repository-owned.

Likely candidates after Wave 1 are queue algorithms, timer-service design,
serial multiplexing, and selected driver components. Whole-system build logic,
SDF generation, and service topology are not adopted now.

## License and maintenance posture

LionsOS identifies itself as active research and development. Its source uses
per-file SPDX declarations, commonly BSD-2-Clause for code and CC-BY-SA-4.0 for
documentation. Any reuse must preserve the individual file's SPDX license;
there is no blanket assumption that every path has the same terms.

Depending directly on upstream `main` would also make this repository's
reproducibility depend on a fast-moving research tree. Any future import should
pin a commit and copy or wrap only the required component, with an update note
and QEMU regression job.

## Decision consequences

**Adopt now:** the sDDF separation-of-concerns model, explicit queue ownership,
and the practice of thin device-facing components.

**Evaluate later:** individual network/timer/serial components after this
repository moves to a compatible Microkit release and has a Rust/C adapter
proof story.

**Decline for Wave 1:** replacing the build system, generated system topology,
protocol crates, or existing products with LionsOS wholesale.

This decision does not block any other Wave 1 package. It minimizes interface
churn while leaving a clear, testable condition for reopening the substrate
choice.
