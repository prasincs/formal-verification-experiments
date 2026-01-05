# Microkit RPi4B PSCI Boot Fix

This document describes the debugging process and fix for a boot failure when running
seL4 Microkit on Raspberry Pi 4B.

## Problem Summary

**Symptoms**: After U-Boot loads the Microkit system and jumps to it with `go 0x10000000`,
the system immediately crashes with an exception.

**Serial Output**:
```
LDR|INFO: disabling MMU (if it was enabled)
LDR|ERROR: loader trapped exception: Synchronous (Current Exception level with SP_ELx)
esr_el2: 0x0000000002000000
ec: 0x00000000 (Unknown reason)
```

**Environment**:
- Raspberry Pi 4 Model B (8GB)
- Microkit SDK 2.1.0 (released Nov 26, 2025)
- U-Boot v2025.10

## Debugging Timeline

### 1. Serial Connection Issues

Initial attempts to see serial output failed. The solution was:
- Add `core_freq=250` to config.txt (stabilizes UART clock)
- Swap TX/RX wires (they were reversed)
- Baud rate: 115200

### 2. Memory Size Discovery

Serial output from U-Boot revealed the Pi has 8GB RAM, not 2GB as initially assumed:
```
DRAM:  Bank 0: 0x0 - 0x40000000 (1GB)
       Bank 1: 0x40000000 - 0x1_00000000 (3GB)
       Bank 2: 0x1_00000000 - 0x2_00000000 (4GB)
       Total: ~8GB
```

This required rebuilding for `rpi4b_8gb` instead of `rpi4b_2gb`.

### 3. seL4Test Comparison

Built and ran seL4Test to verify the hardware works:
```bash
cd sel4test
mkdir build && cd build
../init-build.sh -DPLATFORM=rpi4 -DAARCH64=1
ninja
```

**Result**: All 140 seL4Test tests passed! This proved:
- U-Boot `go` command works correctly
- seL4 kernel boots fine on this hardware
- The bug is specific to Microkit's loader, not seL4 itself

### 4. Root Cause Analysis

Searched GitHub issues and found:
- **Issue #401**: "BCM2711 (RPi4) boot failure in Microkit 2.1.0"
- **PR #402**: "loader: hot-fix for RPi4B" (merged Nov 28, 2025)

The fix was merged **2 days after** SDK 2.1.0 was released.

### 5. The Bug

In `loader/src/aarch64/init.c`, the loader makes a PSCI SMC call:

```c
uint32_t ret = arm_smc32_call(PSCI_FUNCTION_VERSION, 0, 0, 0);
```

This call works on most ARM64 platforms but fails on RPi4B because:
- The Pi's firmware runs in EL3 (secure world)
- It doesn't properly handle PSCI SMC calls from EL2
- The SMC triggers an exception that crashes the loader

### 6. The Fix (PR #402, commit 23f1f6d)

```c
// TODO: handle non-PSCI platforms better, see https://github.com/seL4/microkit/issues/401.
#if !defined(CONFIG_PLAT_BCM2711)
    uint32_t ret = arm_smc32_call(PSCI_FUNCTION_VERSION, 0, 0, 0);
    if (ret != PSCI_VERSION(1, 0) && ret != PSCI_VERSION(1, 1)) {
        LOG_LOADER_ERR("PSCI version mismatch");
        return;
    }
#endif
```

The fix simply skips the PSCI version check on BCM2711 (RPi4) platforms.

## Building the Fixed SDK

Since SDK 2.1.0 doesn't include the fix, we built from source:

```bash
# Add Microkit as submodule
git submodule add https://github.com/seL4/microkit.git vendor/microkit

# Build using Docker with seL4 test tools
docker run --rm -u $(id -u):$(id -g) -e CCACHE_DISABLE=1 \
  -v $(pwd):/workspace \
  -w /workspace/rpi4-graphics/vendor/microkit \
  trustworthysystems/camkes python3 build_sdk.py \
    --sel4 /workspace/sel4test/kernel \
    --boards rpi4b_8gb \
    --configs debug \
    --skip-tool \
    --skip-initialiser \
    --skip-docs \
    --skip-tar \
    --gcc-toolchain-prefix-aarch64 aarch64-linux-gnu
```

### Build Notes

- **`--skip-tool`**: We use the existing tool from SDK 2.1.0 (it's fine)
- **`--skip-initialiser`**: Skip Rust component (requires Rust 1.88+)
- **`--gcc-toolchain-prefix-aarch64 aarch64-linux-gnu`**: The Docker image has
  `aarch64-linux-gnu-gcc` but not `aarch64-none-elf-gcc`
- **`-e CCACHE_DISABLE=1`**: Avoids permission errors with ccache

### Docker Image Issues Encountered

1. **ccache permission denied**: Solved with `-e CCACHE_DISABLE=1`
2. **No aarch64-none-elf-gcc**: Used `--gcc-toolchain-prefix-aarch64 aarch64-linux-gnu`
3. **Clang 11 too old**: `-mno-outline-atomics` requires Clang 15+; used GCC instead

## Files Changed

The fix only affects one file in the loader:

```
loader/src/aarch64/init.c
```

The change adds a `#if !defined(CONFIG_PLAT_BCM2711)` guard around the PSCI check.

## Testing

After applying the fix:

1. Build the graphics project:
   ```bash
   make MICROKIT_BOARD=rpi4b_8gb
   ```

2. Create SD card image:
   ```bash
   make sdcard-uboot MICROKIT_BOARD=rpi4b_8gb
   ```

3. Flash and boot:
   ```bash
   sudo dd if=build/rpi4-sel4-full.img of=/dev/sdX bs=4M status=progress conv=fsync
   ```

Expected serial output should no longer show the PSCI exception.

## Timeline Summary

| Date | Event |
|------|-------|
| Nov 26, 2025 | Microkit SDK 2.1.0 released |
| Nov 28, 2025 | PR #402 merged (RPi4B fix) |
| Jan 4, 2026 | We built SDK from source with fix |

## Lessons Learned

1. **Serial debugging is essential**: HDMI output alone wasn't enough to diagnose the issue
2. **Check recent commits**: The fix existed but wasn't in the release
3. **Compare with known-working software**: seL4Test passing proved hardware was fine
4. **Docker for reproducible builds**: Avoided local toolchain version issues

## References

- [Microkit Issue #401](https://github.com/seL4/microkit/issues/401)
- [Microkit PR #402](https://github.com/seL4/microkit/pull/402)
- [Microkit commit 23f1f6d](https://github.com/seL4/microkit/commit/23f1f6d)
