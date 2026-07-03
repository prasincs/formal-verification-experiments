# USB HID Keyboard Input (DWC2)

## Summary

The Input Protection Domain can now receive real key events from a **USB HID
boot-protocol keyboard**, in addition to the existing serial (UART) input path.
Input is driven by the Raspberry Pi 4's on-SoC **DWC2 USB 2.0 OTG controller**
(BCM2711, physical `0xFE98_0000`), entirely from userspace inside the isolated
Input PD — the seL4 microkernel grants that PD a capability to the USB register
window and a dedicated DMA buffer, and nothing else.

Previously the keyboard interface was a placeholder: `Keyboard::poll()` returned
`None` with a `TODO: Read from USB HID endpoint`. This change implements the
transport under it.

## Enabling it

USB keyboard support is **off by default** and gated behind the
`CONFIG_INPUT_USB_KEYBOARD` Kconfig option (cargo feature `usb`); see
`build-configuration.md` for how the configuration system works.

```bash
# tvdemo enables it in its defconfig:
make PRODUCT=tvdemo PLATFORM=rpi4 ISOLATED=1 sdcard

# photoframe is serial-only by default; opt in per build:
make PRODUCT=photoframe PLATFORM=rpi4 CONFIG_INPUT_USB_KEYBOARD=y sdcard
```

Enabling the option does two coupled things: compiles the `usb` feature into
`input_pd`, and keeps the guarded `usb_regs`/`usb_dma` mappings in the
generated `.system` description. With the option off, the Input PD is not
granted the USB MMIO capability at all.

## Where the code lives

| Component | File | Role |
|-----------|------|------|
| DWC2 host controller | `rpi4-input/src/usb/dwc2.rs` | Core reset, host-mode config, root-port reset/speed, host-channel control + interrupt-IN transfers via internal DMA |
| HID / enumeration protocol | `rpi4-input/src/usb/hid.rs` | SETUP-packet builders, configuration-descriptor parsing, boot-keyboard endpoint discovery |
| Driver orchestration | `rpi4-input/src/usb/mod.rs` | `UsbKeyboard`: enumeration state machine, interrupt-IN polling, decode via the existing `Keyboard` HID decoder |
| Input manager integration | `rpi4-input/src/lib.rs` | `InputManager::attach_usb_keyboard`, polled in the input sweep |
| Protection domain wiring | `rpi4-input-pd/src/main.rs` | Best-effort USB bring-up, forwards key events to the shared ring buffer |
| System descriptions | `rpi4-graphics/tvdemo-input.system`, `rpi4-photoframe/photoframe.system` | Map the USB MMIO + DMA regions into the Input PD |

## Data path

```
USB keyboard ──▶ DWC2 core ──▶ host channel (internal DMA)
                                     │  8-byte boot report
                                     ▼
          Keyboard::process_hid_report()  ──▶  KeyEvent
                                     │
                                     ▼
          Input PD write_event() ──▶ shared ring buffer ──▶ Graphics/Photoframe PD
```

1. **Bring-up** (`UsbKeyboard::init`): soft-reset the DWC2 core, force host mode,
   enable internal DMA, size the FIFOs, and power the root port.
2. **Enumeration** (lazy, on first `poll`): reset the port and read the negotiated
   speed, then run the standard control-transfer sequence on EP0 —
   `GET_DESCRIPTOR(device)` → `SET_ADDRESS(1)` → `GET_DESCRIPTOR(config)` →
   `SET_CONFIGURATION` → HID `SET_PROTOCOL(boot)` → `SET_IDLE(0)`. The
   configuration descriptor is parsed to find the interrupt-IN endpoint of a
   HID / boot / keyboard interface.
3. **Polling** (steady state): each `poll` issues one interrupt-IN transfer to the
   keyboard's endpoint. A NAK (idle keyboard) yields `None`; a completed transfer
   delivers the 8-byte boot report to `Keyboard::process_hid_report`, which
   produces a `KeyEvent` (press/release + modifiers), reusing the existing HID
   scancode mapping.

The data toggle (DATA0/DATA1) alternates on each successful interrupt transfer,
and a stall or transaction error drops the driver back to re-enumeration after a
short backoff, so unplugging and replugging the keyboard recovers.

## Memory isolation

When `CONFIG_INPUT_USB_KEYBOARD=y`, the Input PD is granted exactly two new
capabilities, declared in `@if`-guarded blocks of the system descriptions and
reflected in `rpi4-input-protocol`'s `input_pd_can_access` model (which
describes the maximal, USB-enabled configuration):

| Region | Physical | Virtual (Input PD) | Size | Cached |
|--------|----------|--------------------|------|--------|
| DWC2 registers | `0xFE98_0000` | `0x5_0500_0000` | 64 KiB | no |
| USB DMA buffer | `0x3e86_0000` | `0x5_0600_0000` | 4 KiB | no |

The DMA buffer holds only the SETUP packet, the control-transfer data stage
(descriptors), and the interrupt-IN boot report — small, fixed offsets within a
single page. The Graphics / Photoframe PD is **not** granted either region, so
the isolation property is unchanged: the shared input ring buffer remains the
only memory both PDs can touch.

## Scope and validation status

- **What is implemented:** a single low/full-speed boot-protocol HID keyboard on
  the DWC2 **root port**, using the controller's internal DMA engine, with
  blocking (poll-budgeted) transfers.
- **What is not:** split transactions (a low-speed device behind a high-speed
  hub), interrupt-driven (vs. polled) transfers, and the VL805 xHCI controller
  that fronts the Pi 4's USB-A ports over PCIe. A keyboard behind the Pi's
  internal hub therefore needs hub support that this driver does not yet provide.
- **Testing:** the pure logic — register-field packing (`HCCHAR` / `HCTSIZ`),
  interrupt-status decoding, port-speed decoding, SETUP-packet serialization, and
  configuration-descriptor parsing — is covered by host unit tests in the `usb`
  modules (`cargo test -p rpi4-input`). On-hardware bring-up (real timing, a
  specific keyboard) still requires validation on a physical Raspberry Pi 4; the
  register map and transfer sequence follow the DWC2 databook and the Linux and
  Circle bare-metal drivers, but nothing here has been exercised against real
  hardware in CI.

Because bring-up is best-effort, a build running where the DWC2 core never reaches
host mode (for example under QEMU without a USB model) logs the failure and falls
back to UART input rather than faulting.

## References

- Synopsys DWC_otg databook — host mode, internal DMA
- Linux `drivers/usb/dwc2/` (`hw.h` register map, `hcd.c` channel setup)
- Circle bare-metal USB: `lib/usb/dwhcidevice.cpp`, `dwhci.h`
- USB 2.0 specification, chapter 9 (device framework)
- Device Class Definition for HID 1.11, Appendix B (boot protocol)
