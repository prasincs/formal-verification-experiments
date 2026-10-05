# GrapheneOS / Pixel 10a Bring-Up Plan

**Status: plan only — nothing below is implemented beyond what
[android-agent-os.md](android-agent-os.md) already records.** Each
phase names its exit artifact; a phase is done when that artifact is
in the repo, per repo convention.

Goal: run this repo's seL4 agent OS layer (ultimately the `llmdemo`
verified-inference payload) on a **Pixel 10a running GrapheneOS**,
ending with a locked-bootloader device whose trust chain is:

```
Titan M2 + verified boot (own keys, phase 5)
  └─ GrapheneOS (hardened Android host)
       └─ pKVM stage-2 isolation (AVF)
            └─ seL4 + Microkit (this repo)
                 └─ agent PDs (llmdemo, keystore, ...)
```

## Why this device/OS combination

- **Pixel 10a** (released 2026-03-05, $499): Tensor G4 — the same
  AVF/pKVM-capable chip family as the Pixel 9 line, with GrapheneOS
  **production support** and OEM support to 2033. Cheapest currently
  supported AVF device; fine as a dedicated dev unit.
- **GrapheneOS** rather than stock: ships its own fork of the AVF
  Virtualization module/Terminal app that is friendlier to custom VM
  images, and — unlike stock — supports **installing a self-built,
  self-signed OS and re-locking the bootloader**, which is the only
  path to an appliance-grade trust chain without Google's keys.
- **Constraint that shapes everything**: GrapheneOS does not support
  root, by design. The raw `crosvm run` path
  (`scripts/android/run-crosvm.sh`, `make run-avf`) is therefore
  **not usable on GrapheneOS**. All seL4 boot paths here go through
  the supported `VirtualizationService` API instead.

## Hardware / purchase checklist

- [ ] Pixel 10a bought **unlocked from the Google Store** — carrier
      variants may not permit `OEM unlocking`, which the GrapheneOS
      install requires. Verify the toggle works before committing.
- [ ] Treat it as a dedicated dev device until phase 5; the plan
      involves wiping it at least twice (install, own-key reinstall).
- [ ] USB-C data cable + any WebUSB-capable browser host for the
      GrapheneOS web installer.

## Phase 0 — Install GrapheneOS, verify AVF

Standard GrapheneOS web-install flow (unlock bootloader, flash,
**re-lock**), then enable Developer options + USB debugging.

Verify the virtualization stack before anything else:

```
adb shell ls /apex/com.android.virt/bin/        # crosvm, vm present?
adb shell pm list features | grep virtualization
```

Record the AVF APEX (`com.android.virt`) module version — crosvm's
guest-visible machine layout is **not stable across versions**, so
every later artifact (device tree, seL4 platform config) is pinned
against this.

**Exit artifact:** a `docs/` note (or section appended here) with the
device fingerprint, GrapheneOS release, AVF APEX version, and the
output of the two commands above.

## Phase 1 — On-device dev loop via Termux QEMU (works today)

No new code. Install Termux (GitHub/F-Droid build), then:

```
make PRODUCT=llmdemo PLATFORM=android-avf termux-bundle
# push, unpack, pkg install qemu-system-aarch64-headless, run
```

Acceptance: the llmdemo boot log on-device shows the same pinned
`TOKENS SHA256` receipt CI pins on the host. This gives a
phone-local red/green loop for all payload work in later phases.

**Exit artifact:** on-device boot log committed under
`sel4-microkernel/docs/receipts/` (or linked from this file).

## Phase 2 — Custom *Linux* VM via VirtualizationService; capture ground truth

Before any seL4 porting, learn the no-root custom-VM path with a
guest that is guaranteed to boot (Linux), and extract the data the
port needs:

1. Run a custom Linux VM through the GrapheneOS Terminal fork or
   [koiTerminal](https://github.com/outlawsanzhang/koiTerminal)
   (custom-image Terminal fork of the GrapheneOS repo), and/or the
   VM launcher app with
   `adb shell pm grant <launcher> android.permission.USE_CUSTOM_VIRTUAL_MACHINE`
   and a `vm_config.json` naming a custom kernel. Confirm which of
   these GrapheneOS permits on a locked user build — this is the
   plan's **biggest open question** (GrapheneOS hardening may block
   development-permission grants; if so, skip to the phase-5 fallback
   for phases 4+ and continue).
2. From inside that Linux guest, dump crosvm's generated device tree
   (`/sys/firmware/devicetree` → `dtc`). This is the **ground truth**
   for the seL4 platform port: RAM base, GIC (v3) addresses, UART,
   timer, PSCI conduit, virtio-mmio layout.
3. Note how the kernel is handed to crosvm by VirtualizationService
   (arm64 `Image` header expected — confirm), since that decides how
   the Microkit loader image must be wrapped.

**Exit artifact:** the dumped `.dts` committed under
`sel4-microkernel/docs/avf/` with the AVF APEX version it came from,
plus a short note answering (1) and (3).

## Phase 3 — seL4 kernel platform + Microkit board for crosvm

The upstream work sized in [android-agent-os.md](android-agent-os.md)
("What closing the gap requires"), now against the phase-2 DTS
instead of documentation:

1. seL4 platform `crosvm` (BSP-class port: GICv3, 8250 UART, arch
   timer, PSCI-HVC, **EL1 entry / non-hyp config** — KVM guests get
   no EL2). Upstream to seL4/seL4.
2. Microkit board `crosvm_aarch64` (`build_sdk.py` entry;
   `loader_link_address` per the phase-2 layout; loader UART).
   Upstream to seL4/microkit; until released, build the SDK from
   source and re-pin our own SDK artifact in
   `build-system/config/versions.mk` + CI.
3. Repo side: give `android-avf` a board switch
   (`AVF_BOARD ?= qemu_virt_aarch64 | crosvm_aarch64`), keeping the
   QEMU-board default so the Termux/parity venues stay intact; wrap
   the crosvm-board image with the arm64 `Image` header if phase 2
   confirmed it is required.

Validation without hardware in the loop: QEMU can approximate but not
reproduce crosvm's layout, so this phase's red/green loop is
`hello` serial output on the device via the phase-2 launch path.

**Exit artifact:** `hello` serial output from a crosvm VM on the
Pixel 10a; branch links for the two upstream PRs; CI building the
crosvm-board image.

## Phase 4 — llmdemo as the guest on GrapheneOS (no root)

- Launch the crosvm-board `llmdemo` image through the phase-2
  launcher path; route the guest console to the launcher's log.
- Acceptance: same pinned receipt as phase 1, now under pKVM instead
  of QEMU — at that point the agent payload is running isolated from
  Android on a locked, unrooted GrapheneOS device.

**Exit artifact:** receipt log + `docs/android-agent-os.md` status
table flipped for the crosvm venue.

## Phase 5 — Own-key GrapheneOS build (appliance endgame)

For the RFC's "personal trust anchor" claim, remove the adb-granted
permission and the sideloaded launcher from the story:

- Build GrapheneOS from source with our VM-launcher app as a
  privileged system component holding the VM permissions, signed
  with our own keys, bootloader re-locked → the full chain at the
  top of this file, attestable end to end.
- Then: AVF **protected VM** packaging (pvmfw: AVB-signed kernel via
  `avbtool add_hash_footer`, arm64 header, DTB handoff) so Android
  itself cannot read guest memory, and map AVF/DICE attestation onto
  the RFC's measured-boot chapter (tracked there as future work).

This phase is deliberately unsized; it starts only after phase 4.

## Risks and open questions

| # | Risk | Impact | Probe |
|---|---|---|---|
| 1 | GrapheneOS blocks `pm grant` of `USE_CUSTOM_VIRTUAL_MACHINE` on locked builds | Phases 2/4 need the phase-5 own-build route earlier | Phase 2, step 1 — cheap to test first |
| 2 | VirtualizationService only accepts arm64 `Image`-format kernels | Loader needs header wrapper (small, known work) | Phase 2, step 3 |
| 3 | crosvm layout drifts across AVF APEX updates | seL4 platform pinned per APEX version; re-dump DTS on update | Record version in phases 0/2; re-check after OTAs |
| 4 | Upstream review latency (seL4/Microkit PRs) | Carry forked SDK longer; CI pins self-built SDK | Phase 3 |
| 5 | Tensor G4 AVF restricts non-Microdroid guests in ways docs don't state | Found only on hardware | Phase 2 is designed to surface this before any porting |

## Non-goals (for this plan)

- Rooting the device or unlocked-bootloader daily use — contrary to
  the point of GrapheneOS and the appliance story.
- GPU/display for the guest; serial/virtio-console only.
- Replacing the RPi4/TPM track — this is a parallel deployment
  vehicle, per the RFC.
