# Session Handoff: USB keyboard input + Kconfig build configuration

> Working notes for continuing this branch in a fresh session.
> **Delete this file before merging the branch.**

## Where things stand

Branch `claude/usb-keyboard-io-22j4sp`, two commits, **all CI green** on both
(including the TV Demo / Photo Frame SD-card image builds, which compile
`input_pd` against the real Microkit SDK — see "What is and isn't verified").

| Commit | What it did |
|--------|-------------|
| `2af37fb` | Implemented USB HID keyboard input via the DWC2 host controller. Replaced the `Keyboard::poll()` stub (`TODO: Read from USB HID endpoint`) with a real transport. |
| `1ef9b3f` | Gated USB behind cargo features and built a Kconfig-style configuration system that drives both the features and the `.system` device mappings. |

Original request: *"Implement IO like usb keyboard to use"*, then
*"Put usb behind a build feature. Can we create a kconfig style configuration
system?"* — both done.

## The immediate next task (user-requested, not started)

The user challenged the choice of POSIX sh + awk for the kconfig tool
(`build-system/scripts/kconfig.sh`) and the assistant recommended and offered a
**rewrite as a small std-only Rust host crate**. The user has not explicitly
said "go" yet — confirm before starting, then:

- New host crate (suggested: `sel4-microkernel/build-system/kconfig-tool/`),
  **zero dependencies** (std only, so no network at build time), builds with
  stable Rust. Two subcommands matching the existing CLI exactly:
  - `resolve --kconfig F --defconfig F [--set CONFIG_X=y|n]... --out-config F --out-mk F`
  - `gensystem --config F --in F --out F`
- **Keep the entire contract identical**: Kconfig/defconfig file formats,
  `.config` + `config.mk` output formats, `@if`/`@endif` marker semantics,
  error-message prefix `kconfig: error:` (test-kconfig.sh greps for it),
  content-stable writes (only rewrite output files when content changes —
  make rules depend on `.config` mtime).
- Bootstrap wrinkle: `config/kconfig.mk` runs the resolver at **make parse
  time** via `$(shell ...)`, before any rule runs. The plan: build the tool
  lazily inside the `$(shell)` (`cargo build -q --release` in the tool dir —
  ~100ms cached no-op after first build), then invoke the binary. Keep
  `kconfig.sh` as a thin wrapper or delete it and update all call sites:
  `config/kconfig.mk`, `scripts/test-kconfig.sh`, `.github/workflows/ci.yml`
  (kconfig job), docs (`build-system/README.md`,
  `docs/build-configuration.md`).
- Port the 48 shell self-test cases to `#[test]`s; the CI `kconfig` job then
  runs `cargo test` in the tool dir instead of `test-kconfig.sh` (or keep the
  shell harness as an end-to-end CLI test — assistant's suggestion was to swap
  the CI step for `cargo test`).

## What was built (file map)

### USB HID keyboard (commit `2af37fb`)

| Path | Content |
|------|---------|
| `sel4-microkernel/rpi4-input/src/usb/dwc2.rs` | DWC2 (BCM2711 USB OTG, phys `0xFE980000`) host driver: core reset, host mode, root-port reset/speed, host-channel control + interrupt-IN transfers via internal DMA. Register map follows Linux `dwc2/hw.h`; Circle bare-metal was the other reference. |
| `sel4-microkernel/rpi4-input/src/usb/hid.rs` | SETUP-packet builders, config-descriptor walker that finds a boot-keyboard interrupt-IN endpoint. Pure logic, well tested. |
| `sel4-microkernel/rpi4-input/src/usb/mod.rs` | `UsbKeyboard`: lazy enumeration state machine (GET_DESCRIPTOR → SET_ADDRESS(1) → SET_CONFIGURATION → SET_PROTOCOL(boot) → SET_IDLE(0)), one interrupt-IN read per `poll()`, NAK = idle, stall/error → backoff + re-enumerate. Reports decode via the pre-existing `Keyboard::process_hid_report`. |
| `sel4-microkernel/rpi4-input-pd/src/main.rs` | Input PD: best-effort USB bring-up (falls back gracefully if the core never reaches host mode, e.g. QEMU), forwards `KeyEvent`s to the verified ring buffer. |
| `sel4-microkernel/rpi4-input-protocol/src/lib.rs` | Isolation model (`input_pd_can_access`) extended with the USB MMIO + DMA regions (documents the maximal, USB-enabled config). |
| `sel4-microkernel/docs/usb-keyboard-input.md` | Full driver doc: data path, memory isolation, scope/limitations. |

Known limitations (documented, deliberate): root-port device only — **no split
transactions** (keyboard behind a high-speed hub won't work), no VL805 xHCI
(the Pi 4's USB-A ports), polled not interrupt-driven. **Never validated on
physical hardware** — register sequences follow the databook/Linux/Circle but
timing on a real Pi 4 with a real keyboard is unproven.

### Feature gating + Kconfig (commit `1ef9b3f`)

| Path | Content |
|------|---------|
| `rpi4-input/Cargo.toml` | `usb` feature, **off by default**; gates `pub mod usb` + `InputManager` USB hooks in `lib.rs`. |
| `rpi4-input-pd/Cargo.toml` | `uart` (in default features — bare `cargo build` stays UART-only and CI-compatible) and `usb` features; `main.rs` is cfg-gated per source. |
| `build-system/Kconfig` | Option declarations: `CONFIG_INPUT_UART` (default y), `CONFIG_INPUT_USB_KEYBOARD` (default n). Language subset: `config`/`bool`/`default`/`depends on` (`&&`, `!`)/`help`/`menu`. |
| `build-system/configs/{tvdemo,photoframe}_defconfig` | tvdemo: USB **on**. photoframe: USB **off** (user wanted photoframe minimal). |
| `build-system/scripts/kconfig.sh` | The sh+awk tool (subject of the pending rewrite). `resolve` layers defaults ← defconfig ← `make CONFIG_X=y` overrides with hard-error validation; `gensystem` strips/keeps `<!-- @if CONFIG_X -->` blocks in `.system` files (nesting + `!` negation supported; unknown option or unbalanced markers = build failure). |
| `build-system/config/kconfig.mk` | Make glue: parse-time resolution, maps `CONFIG_INPUT_*` → `--no-default-features --features uart,usb` on the `input_pd` build (target-specific `CARGO_BUILD_STD +=`, same pattern as networking.mk), swaps `SYSTEM_DESC` to the generated file. No-op for products without a defconfig. |
| `build-system/scripts/test-kconfig.sh` | 48-check self-test incl. the real repo defconfigs and all three `.system` templates. |
| `.system` files | `tvdemo-input.system`, `photoframe.system`, **and `tvdemo-network.system`** all carry `@if CONFIG_INPUT_USB_KEYBOARD`-guarded `usb_regs` (64KiB @ `0xFE980000`) + `usb_dma` (4KiB @ phys `0x3e860000`, vaddrs `0x5_0500_0000`/`0x5_0600_0000`) blocks. tvdemo-network needed it too or a USB-enabled input_pd would fault on unmapped MMIO in the three-PD build. |
| `.github/workflows/ci.yml` | New jobs: `kconfig` (runs test-kconfig.sh), `input-library` (cargo test --all-features + both feature builds of rpi4-input). |
| `docs/build-configuration.md` | Full design doc incl. "Adding an option" recipe. |

### The key design invariant (preserve in any rewrite)

One `CONFIG_` option controls **both** what code is compiled **and** what MMIO
the kernel maps into the PD, derived from the same `.config`. So a driver is
never compiled without its mapping (fault), and a PD is never granted device
MMIO for a driver that isn't compiled in (least privilege — this repo's whole
security story). Also: the checked-in `.system` templates remain valid Microkit
files on their own because the markers are XML comments (standalone/manual
builds get the USB-enabled maximal config; code compiled without `usb` simply
never touches those regions).

## What is and isn't verified

Verified:
- `cargo test --all-features` in `rpi4-input`: 14 unit + 2 doc tests.
- Builds in every feature combination, host and aarch64 cross
  (`--target .../aarch64-sel4-microkit.json -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem -Z json-target-spec`).
- `test-kconfig.sh`: 48/48.
- Make integration via `make -n` dry runs: photoframe default → `--features
  uart`; tvdemo ISOLATED default → `--features uart,usb`; command-line
  overrides flip features and regenerate the `.system`; unchanged config
  doesn't churn rebuilds; `hello`/`graphics`/`netdemo` (no defconfig)
  untouched.
- **CI green on `1ef9b3f` across all five workflows** — crucially the TV Demo
  and Photo Frame image builds compile `input_pd.elf` with the real SDK, which
  could not be done locally.

NOT verified:
- Anything on physical hardware. The DWC2 bring-up (port reset timing,
  a real keyboard's enumeration quirks) is untested outside code review.
- QEMU boot behavior with USB enabled (expected path: `init()` fails
  gracefully → UART fallback; worth a boot test someday).

## Environment gotchas (this remote container)

- **No Microkit SDK locally** → `rpi4-input-pd` and other PD crates fail in
  the `sel4-config-data` build script with `SEL4_INCLUDE_DIRS or SEL4_PREFIX
  must be set`. That's environmental, not a code error. The `rpi4-input`
  *library* builds fine. CI has the SDK.
- Pinned toolchain `nightly-2026-07-02` via `sel4-microkernel/rust-toolchain.toml`;
  rustup auto-installs it. `.json` target specs need `-Z json-target-spec` on
  this nightly.
- The `rpi4-input` crate is **not rustfmt-clean** (pre-existing); only the new
  `usb/*.rs` files were formatted. Repo CI only fmt-checks `verified/` and
  `verus/`. Don't run `cargo fmt` across the whole crate.
- `make` warnings about "overriding recipe for target ...photoframe_pd.elf"
  are pre-existing (generic rust.mk rule vs product rule), not from this work.
- GitHub access via MCP tools only (no `gh` CLI). Push with
  `git push -u origin claude/usb-keyboard-io-22j4sp`.

## Backlog / follow-up ideas (beyond the Rust rewrite)

1. Migrate `NET_DRIVER` / `NET_STACK` / `ISOLATED` make vars into Kconfig
   options (docs already flag this as the natural next step; left alone to
   avoid churning networking CI).
2. Hardware validation of the USB driver on a real Pi 4 (root-port keyboard,
   USB-C/OTG port).
3. Split-transaction support (keyboard behind a hub) and/or xHCI for the
   USB-A ports — both explicitly out of scope so far.
4. Interrupt-driven USB polling (currently the PD polls on notifications).
5. QEMU boot test asserting the graceful USB-init-failure → UART fallback.

## Quick command reference

```bash
# kconfig self-tests
./sel4-microkernel/build-system/scripts/test-kconfig.sh

# input library tests / feature builds
cd sel4-microkernel/rpi4-input
cargo test --all-features && cargo build --no-default-features && cargo build --features usb

# make-level checks (no SDK needed for these)
cd sel4-microkernel/build-system
make info PRODUCT=photoframe PLATFORM=rpi4
make -n PRODUCT=tvdemo PLATFORM=rpi4 ISOLATED=1 \
  $(pwd)/../build/rpi4/tvdemo/input_pd.elf | grep features
make PRODUCT=photoframe PLATFORM=rpi4 CONFIG_INPUT_USB_KEYBOARD=y \
  $(pwd)/../build/rpi4/photoframe/photoframe.system
```
