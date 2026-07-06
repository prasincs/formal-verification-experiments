# Android Target: seL4 as a Secure Agent OS Layer

**Status: build + Termux/QEMU path working; AVF/crosvm boot is staged,
not working — it requires a Microkit board port (gap documented
below).** Claims here follow the repo convention: anything stated as
working is backed by an artifact in this repo; everything else is
labeled as a gap.

This document describes the `android-avf` platform target: running the
seL4 Microkit system images from this repo (e.g. the [`llmdemo`
verified-inference product](llm-roadmap.md)) as an **isolated guest OS
layer on an Android device**, underneath and outside the Android TCB,
using the Android Virtualization Framework (AVF). It complements the
[agent appliance design RFC](secure-agent-os.md): AVF-capable phones
are a candidate *deployment vehicle* for the same appliance payloads,
with pKVM providing the stage-2 isolation that the RFC gets from
running seL4 on bare metal.

## Why this target

Android 14+ devices with AVF (recent Pixels, supported Snapdragon
boards) ship `crosvm` as the VMM inside the `com.android.virt` APEX.
The standard AVF flow boots Microdroid (a stripped Android) as the
guest; bypassing Microdroid and booting a custom kernel payload gives
us a path to run seL4 — and the agent workloads in this repo — on
commodity phone hardware:

- **Isolation**: the guest runs under KVM (pKVM on protected-VM
  devices); Android itself cannot address guest memory in the pKVM
  case. Inside the guest, seL4's capability model isolates the agent
  PDs from each other as on every other platform in this repo.
- **Deployment**: phones are the hardware people already carry. An
  attestable agent layer on a phone is a more plausible "personal
  trust anchor" form factor than a dedicated RPi4 appliance.
- **Dev loop**: Termux QEMU gives an on-device software-emulation loop
  with no root and no Android permissions ceremony.

## One image, three execution venues

`PLATFORM=android-avf` builds the **same `qemu_virt_aarch64` Microkit
board image** as `qemu-aarch64`. That is deliberate:

| Venue | Command | Status |
|---|---|---|
| Host QEMU (parity check) | `make PRODUCT=hello PLATFORM=android-avf run` | ✅ works — identical machine to `qemu-aarch64` |
| Termux QEMU on the device | `make ... termux-bundle`, unpack in Termux, `sh run-termux-qemu.sh` | ✅ works wherever Termux QEMU installs — same `virt` machine, software-emulated |
| AVF crosvm | `make ... run-avf` | ⚠️ deployment scripted; **guest will not boot** until the board-port gap below is closed |

Because venues 1 and 2 emulate the exact board the image is linked
for, anything validated there (e.g. the deterministic `llmdemo`
inference receipts) transfers to the device unchanged; only venue 3
has remaining porting work.

## Usage

```bash
cd sel4-microkernel/build-system

# Build (identical artifact to PLATFORM=qemu-aarch64, separate build dir)
make PRODUCT=llmdemo PLATFORM=android-avf

# Sanity-boot the image on the host first
make PRODUCT=llmdemo PLATFORM=android-avf run

# On-device software emulation (no root):
make PRODUCT=llmdemo PLATFORM=android-avf termux-bundle
adb push ../../build/android-avf/llmdemo/termux-llmdemo.tar.gz /sdcard/Download/
# in Termux:  pkg install qemu-system-aarch64-headless
#             tar xzf /sdcard/Download/termux-llmdemo.tar.gz
#             sh run-termux-qemu.sh          # Ctrl-A X exits

# AVF path (rooted or userdebug device):
make PRODUCT=llmdemo PLATFORM=android-avf deploy-avf   # adb push + capability check
make PRODUCT=llmdemo PLATFORM=android-avf run-avf      # boots crosvm, serial -> terminal
```

Tunables (see `config/platforms/android-avf.mk`): `ADB`,
`ANDROID_SERIAL`, `ANDROID_STAGE_DIR`, `CROSVM_BIN`, `AVF_MEMORY_MB`,
`AVF_CPUS`.

### Device requirements for the crosvm path

- Android 14+ with the AVF virtualization APEX
  (`/apex/com.android.virt/bin/crosvm` present). `deploy-avf` checks
  and reports this.
- A **root shell** (rooted device, or userdebug build where `adb root`
  works). Running crosvm directly — outside `VirtualizationService` —
  is not permitted for the shell domain on user builds.
- The launcher passes `--disable-sandbox`: crosvm's own seccomp
  sandbox policies are keyed to being spawned by
  `VirtualizationService` and kill the VMM's worker threads when it is
  launched from an interactive shell. Note the trade-off: sandbox off
  means a compromised VMM process is confined only by SELinux and
  Unix permissions, while the *guest* remains confined by KVM/pKVM
  stage-2 translation regardless.
- Serial: crosvm routes the guest UART to stdout, i.e. into the adb
  shell session — `run-avf` streams boot output into your terminal.

## crosvm bring-up status (the honest part)

An earlier survey of this approach claimed that because crosvm and
QEMU both provide an ARM `virt`-style machine, code tested under QEMU
needs "virtually no hardware abstraction layer changes" on crosvm.
**That is true for Linux guests, which discover hardware from the
devicetree crosvm generates at runtime. It is not true for seL4**: the
kernel and the Microkit loader are configured against a *static*
platform description at build time. The two machines differ in
exactly the places that are baked in:

| Property | QEMU `virt` (our image) | crosvm aarch64 |
|---|---|---|
| RAM base | `0x4000_0000` | `0x8000_0000` |
| UART | PL011 @ `0x0900_0000` | 8250-compatible |
| Interrupt controller | GICv2 (QEMU default) | GICv3 |
| Guest load protocol | `-device loader,addr=0x7000_0000` (raw image at link address) | `--bios` places the image at RAM base; no arbitrary-address loader |
| Hardware description | consumed at seL4 *build* time | generated devicetree, consumed at *runtime* (by Linux) |

Consequently `run-avf` with today's image is expected to produce **no
guest output** — the loader starts (if at all) at the wrong address,
against the wrong UART and GIC. The launcher script says so at the
top rather than pretending otherwise.

### What closing the gap requires

1. **seL4 kernel platform for crosvm's machine**: a devicetree/
   platform config with RAM at `0x8000_0000`, GICv3, the 8250 UART,
   and crosvm's virtio-mmio layout. seL4 already supports GICv3 and
   8250-class UARTs on other platforms, so this is configuration and
   bring-up work, not new kernel code.
2. **Microkit board definition** (`crosvm_aarch64`) wrapping that
   platform, with the loader link address chosen to match where
   `--bios` places the image (RAM base), since crosvm has no
   arbitrary-address loader device.
3. **Serial + timer validation** under `run-avf` on a rooted AVF
   device, then re-running the product acceptance tests (llmdemo
   receipts) there.

Until then, the supported on-device path is Termux QEMU, which needs
no port because it *is* the build's target machine.

## Relation to the agent-appliance work packages

- The [design RFC](secure-agent-os.md) targets RPi4-class hardware
  with a TPM; `android-avf` swaps the hardware root of trust story —
  AVF protected VMs get measured boot and attestation from pKVM +
  Android's KeyMint/AVF attestation instead of a discrete TPM. Mapping
  the RFC's attestation chapter onto AVF attestation is future design
  work, not covered here.
- Workloads are unchanged: any product buildable for
  `qemu-aarch64` is a candidate payload (currently `hello` and
  `llmdemo` are enabled for `android-avf`; others can be added by
  extending their platform filter once they have no RPi4-only device
  dependencies).

## Follow-ups

- [ ] Microkit `crosvm_aarch64` board port (items 1–2 above).
- [x] CI: build `PRODUCT=llmdemo PLATFORM=android-avf` and pin
      byte-parity with the QEMU-booted `qemu-aarch64` image
      (`.github/workflows/llmdemo.yml`, "check venue parity" step).
- [ ] Evaluate the supported AVF "custom VM" config path
      (`vm run` / `VirtualizationService` with a raw kernel) as a
      non-root alternative on devices where it is enabled.
- [ ] AVF attestation mapping for the RFC's measured-boot chapter.
