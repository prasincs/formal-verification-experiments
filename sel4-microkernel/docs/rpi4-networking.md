# Raspberry Pi 4 Networking on seL4

This document describes networking options for seL4 Microkit on Raspberry Pi 4, comparing Ethernet and WiFi approaches, and explaining the compile-time configuration system.

## Hardware Overview

The Raspberry Pi 4 Model B includes two networking interfaces:

| Interface | Chip | Connection | Max Speed |
|-----------|------|------------|-----------|
| **Ethernet** | BCM54213PE (GENET) | Native SoC bus | 1 Gbps |
| **WiFi** | CYW43455 | SDIO (4-bit) | ~160 Mbps theoretical |

## Comparison: Ethernet vs WiFi

### Ethernet (BCM54213PE) - Recommended

**Advantages:**
- Native SoC connection (no USB/PCIe bridge)
- Simpler driver architecture (~600 lines in sDDF)
- Full gigabit speeds achievable
- No firmware blob loading required at runtime
- No wireless regulatory compliance complexity
- Lower power consumption
- Deterministic latency (critical for real-time systems)

**Disadvantages:**
- Requires physical cable
- Less flexible deployment

**Driver Complexity:** Medium
- GENET (Gigabit Ethernet) controller driver
- UniMAC MDIO bus controller
- BCM54213PE PHY initialization

**References:**
- [Circle bare metal driver](https://github.com/rsta2/circle/blob/master/lib/bcm54213.cpp)
- Linux `drivers/net/ethernet/broadcom/genet/`

### WiFi (CYW43455) - Advanced

**Advantages:**
- Wireless connectivity
- Flexible deployment
- Bluetooth included (shared chip)

**Disadvantages:**
- Complex SDIO initialization sequence
- Requires proprietary firmware blobs:
  - `brcmfmac43455-sdio.bin` (main firmware)
  - `brcmfmac43455-sdio.txt` (NVRAM config)
  - `brcmfmac43455-sdio.clm_blob` (regulatory database)
- WPA/WPA2 adds significant complexity
- Higher latency variability
- More power consumption
- 802.11 stack overhead

**Driver Complexity:** High
- SDIO bus initialization (Arasan controller)
- Firmware loading and verification
- BCDC protocol implementation
- 802.11 management frames
- WPA supplicant integration

**References:**
- Linux `drivers/net/wireless/broadcom/brcm80211/brcmfmac/`
- NetBSD/OpenBSD `bwfm` driver

## Architecture

### Protection Domain Model

```
┌─────────────────────────────────────────────────────────────┐
│                    seL4 Microkit System                     │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────┐    IPC     ┌──────────────┐              │
│  │  Network PD  │◄──────────►│  Client PD   │              │
│  │              │            │  (Graphics)  │              │
│  │ ┌──────────┐ │            └──────────────┘              │
│  │ │ IP Stack │ │                                          │
│  │ │ (lwIP/   │ │                                          │
│  │ │ picoTCP) │ │                                          │
│  │ └────┬─────┘ │                                          │
│  │      │       │                                          │
│  │ ┌────┴─────┐ │                                          │
│  │ │  Driver  │ │   MMIO                                   │
│  │ │(ETH/WiFi)│ │◄─────────►  Hardware                     │
│  │ └──────────┘ │                                          │
│  └──────────────┘                                          │
│                                                            │
└─────────────────────────────────────────────────────────────┘
```

### Memory Regions

**Ethernet (GENET):**
```
GENET Base:     0xfd580000
GENET Size:     0x10000 (64KB)
```

**WiFi (SDIO):**
```
EMMC2/SDIO:     0xfe340000
SDHOST:         0xfe202000
GPIO (pins 34-39 for SDIO)
```

## Compile-Time Configuration

Networking is enabled via the `NET_DRIVER` build variable:

```bash
# No networking (default)
make PRODUCT=tvdemo PLATFORM=rpi4

# Ethernet only
make PRODUCT=tvdemo PLATFORM=rpi4 NET_DRIVER=ethernet

# WiFi only
make PRODUCT=tvdemo PLATFORM=rpi4 NET_DRIVER=wifi

# Both (Ethernet primary, WiFi secondary)
make PRODUCT=tvdemo PLATFORM=rpi4 NET_DRIVER=both
```

### Feature Flags in Rust

The build system sets Cargo features based on `NET_DRIVER`:

```toml
[features]
default = []
net-ethernet = []
net-wifi = []
net-stack-lwip = []
net-stack-picotcp = []
```

### Conditional Compilation

```rust
#[cfg(feature = "net-ethernet")]
mod ethernet;

#[cfg(feature = "net-wifi")]
mod wifi;

pub fn init_network() {
    #[cfg(feature = "net-ethernet")]
    ethernet::init();

    #[cfg(feature = "net-wifi")]
    wifi::init();
}
```

## IP Stack Options

### lwIP (Recommended)

- Mature, widely used
- ~40KB code size
- Full TCP/IP support
- BSD-style socket API available
- Dual-stack IPv4/IPv6

### picoTCP

- Modular design
- Good for constrained environments
- seL4 fork maintained by Trustworthy Systems
- GPL v2/v3 licensed

## Implementation Roadmap

### Phase 1: Ethernet Driver (Recommended First)
1. Implement BCM54213PE PHY initialization
2. Implement GENET controller driver
3. Integrate with lwIP
4. Create Network PD with IPC interface

### Phase 2: WiFi Driver (Optional)
1. Implement SDIO bus driver (Arasan)
2. Firmware blob loading
3. BCDC protocol
4. Basic open network support
5. WPA supplicant integration (complex)

## Security Considerations

### Isolation Benefits
- Network PD runs in separate protection domain
- Capability-based access control
- Buffer overflow in driver cannot compromise kernel
- Network-facing code isolated from trusted components

### Verification Opportunities
- Ring buffer IPC can use Verus verification (like rpi4-input-protocol)
- Packet parsing can be formally verified
- State machine verification for protocol handlers

## References

- [sDDF Network Framework](https://github.com/au-ts/sDDF) - seL4 Device Driver Framework
- [seL4 Microkit SDK](https://docs.sel4.systems/projects/microkit/) - Official documentation
- [Circle BCM54213 Driver](https://github.com/rsta2/circle) - Bare metal reference
- [seL4 picoTCP Fork](https://github.com/seL4/picotcp) - TCP/IP stack
- [lwIP](https://savannah.nongnu.org/projects/lwip/) - Lightweight IP stack
