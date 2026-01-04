# seL4 Boot Debugging on Raspberry Pi 4

## Hardware

- **Board**: Raspberry Pi 4 Model B
- **RAM**: 2GB (confirmed via U-Boot `bdinfo`: 0x80000000)
- **Display**: HDMI (no serial adapter currently)

## What Works

1. **Raspberry Pi firmware boots** - GPU loads start4.elf, fixup4.dat
2. **U-Boot boots and displays on HDMI** - Shows banner, accepts commands
3. **U-Boot can load images from SD card** - `fatload mmc 0 0x10000000 sel4test.img` succeeds
4. **U-Boot has bootelf support** - CONFIG_CMD_ELF=y confirmed

## What Doesn't Work

1. **`go 0x10000000` hangs** - "Starting application at 0x10000000" shows, then nothing
2. **`bootelf` returns silently** - No error, no output, just returns to prompt
3. **Direct boot (no U-Boot) shows rainbow screen** - GPU can't hand off to seL4 directly
4. **LED morse code never blinks** - Suggests code isn't executing at all

## Builds Created

All builds configured for **2GB RPi4** (`-DRPI4_MEMORY=2048` / `RPI4_MEMORY=2gb`):

| File | Description |
|------|-------------|
| `build/sel4test-2gb.img` | Official seL4Test for RPi4 2GB |
| `build/loader.img` | Microkit graphics loader (binary) |
| `build/loader.elf` | Microkit graphics loader (ELF for bootelf) |
| `build/u-boot.bin` | U-Boot bootloader |

## SD Card Images

| Image | Description |
|-------|-------------|
| `build/rpi4-sel4test.img` | U-Boot + seL4Test auto-boot |
| `build/rpi4-sel4-full.img` | U-Boot + Microkit (bootelf + go fallback) |
| `build/rpi4-sel4-direct.img` | Direct boot attempt (failed - rainbow) |
| `build/rpi4-debug.img` | U-Boot with manual command menu |

## Boot Methods Attempted

### 1. U-Boot `go` command
```
fatload mmc 0 0x10000000 sel4test.img
go 0x10000000
```
**Result**: Hangs after "Starting application at 0x10000000"

### 2. U-Boot `bootelf` command
```
fatload mmc 0 0x20000000 loader.elf
bootelf 0x20000000
```
**Result**: Returns silently to prompt, no output

### 3. Direct boot (no U-Boot)
```
# config.txt
arm_64bit=1
kernel=sel4test.img
kernel_address=0x10000000
```
**Result**: Rainbow screen (GPU can't boot kernel)

## Key Findings

1. **Memory mismatch was initial issue** - First built for 4GB/8GB, Pi has 2GB
2. **Rebuilt for 2GB still doesn't boot** - Rules out memory config as sole cause
3. **seL4 outputs to UART, not HDMI** - Can't see debug output without serial
4. **Image format is correct** - Raw ARM64 binary starting with `d3 19 00 90` (adrp instruction)
5. **Load address matches** - IMAGE_START_ADDR=0x10000000 in elfloader config

## Root Cause Hypotheses

1. **Exception Level (EL) issue** - seL4 may need EL2, U-Boot `go` might drop to EL1
2. **Cache state** - Elfloader may expect specific cache configuration
3. **Missing DTB** - ARM64 boot convention passes device tree in x0
4. **UART configuration** - Elfloader may fail early trying to init UART

## Next Steps (With Serial Adapter)

### Hardware Setup
Connect USB-TTL serial adapter to RPi4:
- **TX** → GPIO 15 (RXD, pin 10)
- **RX** → GPIO 14 (TXD, pin 8)
- **GND** → Ground (pin 6)

### Serial Console
```bash
# Linux
screen /dev/ttyUSB0 115200

# Or with minicom
minicom -D /dev/ttyUSB0 -b 115200
```

### Debug Steps
1. Boot with U-Boot, observe full output
2. Run `fatload mmc 0 0x10000000 sel4test.img`
3. Run `go 0x10000000`
4. Observe any output from seL4 elfloader
5. If no output, try with cache flush:
   ```
   dcache flush
   icache flush
   go 0x10000000
   ```

### Expected seL4Test Output (if working)
```
ELF-loader started on CPU: ARM Ltd. Cortex-A72 r0p3
  paddr=[...]
  vaddr=[...]
Bringing up 3 other cpus
Starting node #0 with ACPI
...
Test suite passed. 123 tests passed. 0 tests failed.
```

## References

- [seL4 RPi4 Documentation](https://docs.sel4.systems/Hardware/Rpi4.html)
- [seL4 ELF Loader](https://docs.sel4.systems/projects/elfloader/)
- [Microkit SDK](https://github.com/seL4/microkit)

## Build Commands

### Rebuild seL4Test for 2GB
```bash
cd ../sel4test
rm -rf build && mkdir build && cd build
source ../.venv/bin/activate
../init-build.sh -DPLATFORM=rpi4 -DAARCH64=1 -DRPI4_MEMORY=2048
ninja
```

### Rebuild Microkit for 2GB
```bash
cd rpi4-graphics
export PATH="$HOME/.cargo/bin:$PATH"
make clean
make RPI4_MEMORY=2gb
make RPI4_MEMORY=2gb elf
```

### Create SD Card Images
```bash
# seL4Test with U-Boot
./scripts/create-sel4test-sdcard.sh --2gb

# Microkit with U-Boot
./scripts/create-sdcard-full.sh --uboot

# Flash to SD card
sudo dd if=build/rpi4-sel4test.img of=/dev/sdX bs=4M status=progress conv=fsync
```
