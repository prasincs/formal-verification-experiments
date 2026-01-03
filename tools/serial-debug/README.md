# Serial Debug Tools

A comprehensive debugging toolkit for embedded systems with device profiles for Raspberry Pi 4, STM32, ESP32, and other devices.

## Features

- **Device Profiles**: Built-in profiles with serial settings, boot stages, and error patterns for:
  - Raspberry Pi 4 (BCM2711)
  - STM32 ARM Cortex-M microcontrollers
  - ESP32 series (ESP32, ESP32-S2, ESP32-S3, ESP32-C3)
  - Generic devices

- **Serial Monitor** (requires `serial` feature):
  - Read and analyze serial output from USB-to-serial adapters
  - Boot stage detection with device-specific patterns
  - Error highlighting with suggestions
  - Log file export

- **Boot Partition Analysis** (for devices with boot partitions):
  - Validate boot files and configuration
  - Check required firmware files
  - Analyze config.txt settings

- **Kernel Image Analysis**:
  - Detect kernel format (ARM64 Image, zImage, ELF, raw binary)
  - Check architecture compatibility
  - Validate for target device

## Installation

### Build without serial support (no libudev required)

```bash
cd tools/serial-debug
cargo build --release
```

### Build with serial support (requires libudev on Linux)

```bash
# Install libudev on Debian/Ubuntu
sudo apt-get install libudev-dev

# Build with serial feature
cargo build --release --features serial
```

## Usage

### List supported device profiles

```bash
serial-debug devices list
```

### Show device profile details

```bash
serial-debug devices show rpi4
serial-debug devices show stm32
serial-debug devices show esp32
```

### Monitor serial output (requires `serial` feature)

```bash
# Monitor with auto-detection
serial-debug serial monitor --device rpi4

# Specify port and baud rate
serial-debug serial monitor -p /dev/ttyUSB0 --device rpi4

# Override baud rate
serial-debug serial monitor -p /dev/ttyUSB0 --device rpi4 -b 921600

# Log to file
serial-debug serial monitor -p /dev/ttyUSB0 --device rpi4 -l boot.log
```

### Analyze boot partition (for RPi4)

```bash
# Analyze boot partition structure
serial-debug boot analyze /media/boot --device rpi4

# Validate configuration
serial-debug boot validate /media/boot --device rpi4

# Quick check for required files
serial-debug boot check /media/boot --device rpi4

# Analyze config.txt
serial-debug boot config /media/boot/config.txt
```

### Analyze kernel image

```bash
# Analyze single image
serial-debug image analyze kernel8.img

# Compare multiple images
serial-debug image compare kernel8.img kernel7l.img
```

### Generate debug configuration

```bash
# Generate debug-friendly config.txt for RPi4
serial-debug generate config --device rpi4

# Save to file
serial-debug generate config --device rpi4 -o config.txt

# Generate cmdline.txt
serial-debug generate cmdline --device rpi4
```

## Device Profiles

### Raspberry Pi 4

- Default baud: 115200
- Architecture: AArch64
- Boot stages: GPU Firmware → start.elf → U-Boot → Linux/seL4 → Init
- Error patterns: SD card errors, kernel panics, seL4 faults, filesystem errors

### STM32

- Default baud: 115200
- Architecture: ARM Cortex-M
- Boot stages: Bootloader → HAL Init → RTOS Init → Application
- Error patterns: Hard Fault, MemManage, Bus Fault, Usage Fault, HAL errors

### ESP32

- Default baud: 115200
- Architecture: Xtensa
- Boot stages: ROM Bootloader → Second Stage → Application → WiFi Init
- Error patterns: Guru Meditation, stack overflow, watchdog timeout, flash errors

## Serial Connection for Raspberry Pi 4

Connect a USB-to-serial adapter to the Pi's UART pins:

| Pi GPIO | Pin | USB Adapter |
|---------|-----|-------------|
| GPIO 14 (TXD) | 8 | RX |
| GPIO 15 (RXD) | 10 | TX |
| GND | 6 | GND |

Enable UART in config.txt:
```
enable_uart=1
uart_2ndstage=1
dtoverlay=disable-bt
```

## Integration with seL4 Microkit Projects

This tool is designed to work with the seL4 Microkit projects in this repository:

- `sel4-microkernel/rpi4-graphics/` - HDMI framebuffer demo
- `sel4-microkernel/rpi4-spi-display/` - SPI display with Verus verification

Monitor seL4 boot with:
```bash
serial-debug serial monitor -p /dev/ttyUSB0 --device rpi4
```

The tool recognizes seL4 Microkit boot stages like `MON|` output.

## License

MIT
