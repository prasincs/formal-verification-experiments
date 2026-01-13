# RPI4 Network Protection Domain

Network Protection Domain for seL4 Microkit on Raspberry Pi 4.

## Overview

This crate provides isolated network functionality for seL4 systems on RPi4,
supporting both Ethernet (BCM54213PE) and WiFi (CYW43455) interfaces.

## Features

- **Ethernet Support**: Native Gigabit Ethernet via BCM54213PE PHY
- **WiFi Support**: 802.11ac WiFi via CYW43455 (requires firmware)
- **Compile-time Selection**: Choose driver at build time
- **IP Stack Integration**: Support for lwIP and picoTCP

## Building

```bash
# Ethernet only (recommended)
make PRODUCT=tvdemo PLATFORM=rpi4 NET_DRIVER=ethernet sdcard

# WiFi only
make PRODUCT=tvdemo PLATFORM=rpi4 NET_DRIVER=wifi sdcard

# Both interfaces
make PRODUCT=tvdemo PLATFORM=rpi4 NET_DRIVER=both sdcard
```

## Architecture

```
┌─────────────────────────────────────────────────┐
│               Network PD                         │
│  ┌───────────────────────────────────────────┐  │
│  │              IP Stack                      │  │
│  │         (lwIP / picoTCP)                  │  │
│  └─────────────────┬─────────────────────────┘  │
│                    │                             │
│  ┌─────────────────┴─────────────────────────┐  │
│  │       Network Interface Layer              │  │
│  └──────┬───────────────────────┬────────────┘  │
│         │                       │               │
│  ┌──────┴──────┐         ┌──────┴──────┐       │
│  │  Ethernet   │         │    WiFi     │       │
│  │ BCM54213PE  │         │  CYW43455   │       │
│  └─────────────┘         └─────────────┘       │
└─────────────────────────────────────────────────┘
```

## Comparison: Ethernet vs WiFi

| Feature | Ethernet | WiFi |
|---------|----------|------|
| Speed | 1 Gbps | ~160 Mbps |
| Latency | Low, deterministic | Variable |
| Setup | Plug & play | Firmware + config |
| Complexity | Medium | High |
| Security | Physical only | WPA2/WPA3 |
| Power | Lower | Higher |

**Recommendation**: Use Ethernet for development and most deployments.
WiFi is more complex and requires proprietary firmware blobs.

## Hardware Memory Map

### Ethernet (GENET)
- Base: `0xfd580000`
- Size: 64KB

### WiFi (SDIO)
- Base: `0xfe340000`
- Size: 4KB

## Implementation Status

### Ethernet Driver
- [x] PHY detection and initialization
- [x] MDIO bus interface
- [x] Link status detection
- [ ] DMA ring buffers (TX/RX)
- [ ] Interrupt handling
- [ ] Full packet transmission

### WiFi Driver
- [x] SDIO controller initialization
- [x] Power management
- [ ] Firmware loading
- [ ] BCDC protocol
- [ ] 802.11 management
- [ ] WPA supplicant

## IPC Protocol

The Network PD communicates with clients via shared memory ring buffers:

```
TX Ring: Client → Network (packets to send)
RX Ring: Network → Client (received packets)
```

See `src/protocol.rs` for message definitions.

## References

- [seL4 sDDF](https://github.com/au-ts/sDDF) - Device Driver Framework
- [Circle](https://github.com/rsta2/circle) - Bare metal BCM54213 driver
- [Linux GENET](https://github.com/torvalds/linux/tree/master/drivers/net/ethernet/broadcom/genet)
- [Linux brcmfmac](https://github.com/torvalds/linux/tree/master/drivers/net/wireless/broadcom/brcm80211/brcmfmac)
