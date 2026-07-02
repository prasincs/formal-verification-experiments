# Networking Roadmap

Tracking document for seL4 Microkit networking on Raspberry Pi 4 and QEMU.
Companion to [rpi4-networking.md](rpi4-networking.md) (architecture and
driver comparison). Check items off as they land; each open item lists
where the gap lives in the code.

## Status at a Glance

| Component | Status | Verified by |
|---|---|---|
| Network PD + shared-memory ring protocol | ✅ Done | QEMU boot test (CI) |
| `rpi4-network-protocol` shared crate | ✅ Done | Used by 3 PDs |
| Ethernet driver (BCM54213PE/GENET) | ✅ Code complete | Compile only — **needs hardware** |
| Virtio-net driver (QEMU) | ✅ Done | End-to-end ARP round trip in CI |
| Graphics PD network client (`network` feature) | ✅ Done | Compile + feature-flag wiring |
| IP stack | ❌ Not started | — |
| WiFi driver (CYW43455) | 🟡 Skeleton only | Compile only |
| Formal verification of ring protocol | ❌ Not started | — |

Landed so far on `claude/rpi-sel4-networking-Bbp2s`:
scaffolding + build flags (`08a50ce`), structural fixes (`c57bad9`),
nightly toolchain probes (`633d433`, `dc4fa57`), protocol crate + GENET
DMA + client wiring (`a79bbfc`), virtio-net + CI boot test (`6a2a73f`).

## Phase 1: Foundations — ✅ Complete

- [x] Network PD (`rpi4-network`) with `#[protection_domain]`/`Handler`
      runtime, matching the codebase's PD pattern
- [x] `NetworkDriver` trait abstraction over Ethernet/WiFi/virtio
- [x] Shared `rpi4-network-protocol` crate (TX/RX rings, state header),
      modeled on `rpi4-input-protocol`
- [x] Compile-time driver selection: `NET_DRIVER=ethernet|wifi|both` on
      tvdemo (requires `ISOLATED=1`), `netdemo` product for QEMU
- [x] Three-PD system description (`tvdemo-network.system`) with
      capability-scoped mappings and channels
- [x] Graphics PD client behind `--features network` (auto-enabled by the
      build system when `NET_DRIVER` is set)

## Phase 2: Ethernet Driver (GENET) — ✅ Code Complete

- [x] GENET v5 detection, MDIO bus, BCM54213PE PHY init + autoneg
- [x] UniMAC reset/port-mode/frame-length setup
- [x] TX/RX DMA rings (ring 16, descriptors in on-chip SRAM, buffers in
      the fixed-phys `net_dma` region at `0x3e700000`)
- [x] INTRL2 interrupt handling (RX/TX done, link events)
- [x] Free-running producer/consumer index handling, runt padding,
      RX error validation, buffer recycling

## Phase 3: QEMU + CI Testing — ✅ Complete

- [x] Legacy virtio-mmio net driver (`net-virtio`), transport probe
      across all 32 slots of the QEMU virt machine
- [x] `netdemo` product: Network PD + `netclient_pd` ring-protocol client
- [x] End-to-end CI boot test (`qemu-netdemo` job): ARP probe through the
      TX ring, slirp reply back through the RX ring, IRQ 79 exercised
- [x] Toolchain drift fixes so all CI builds work on current nightlies

## Phase 4: IP Stack — ❌ Next Up

The `net-stack-lwip` / `net-stack-picotcp` cargo features are declared
but empty (`NET_STACK` in the build system selects between stubs).
Recommendation: **smoltcp** instead — pure no_std Rust, fits the
codebase, avoids C FFI in a PD.

- [ ] Decide stack: smoltcp (recommended) vs lwIP vs picoTCP
- [ ] Implement `smoltcp::phy::Device` over `NetworkDriver`
- [ ] Time source for stack timers (e.g. `CNTVCT_EL0`/`CNTFRQ_EL0` —
      no timer driver exists in the PD today)
- [ ] DHCP client (testable against QEMU slirp's built-in DHCP in CI)
- [ ] ICMP echo (CI: ping 10.0.2.2)
- [ ] TCP/UDP socket service exposed to client PDs — extend
      `rpi4-network-protocol` (the `NetRequestType`/`NetResponseType`
      message types are defined but not yet carried over any channel)
- [ ] Retire or implement the `net-stack-lwip`/`net-stack-picotcp`
      feature declarations once the decision is made

## Phase 5: Hardware Validation (RPi4) — ❌ Blocked on Hardware

The GENET driver is register-faithful to Linux `bcmgenet` but has never
touched real silicon. Known assumptions to validate, from code review:

- [ ] GENET IRQ number (`tvdemo-network.system` declares 189 = GIC SPI
      157; unvalidated)
- [ ] `net_dma` phys region `0x3e700000` is actually free (assumed clear
      of the mailbox/framebuffer carve-out — see comment in
      `tvdemo-network.system`)
- [ ] RGMII pad setup: driver only sets `SYS_PORT_CTRL` port mode and
      relies on RPi firmware having configured `EXT_RGMII_OOB_CTRL`
      (`ethernet.rs` ~line 576). If boot skips the firmware path, add the
      EXT block setup from Linux `bcmmii.c`
- [ ] Flow-control thresholds are approximate (`ethernet.rs` ~line 193;
      only affects pause frames)
- [ ] MDIO/reset delays use spin loops, not calibrated timers
- [ ] TX/RX under load: ring wrap, multi-frame bursts, link flap
- [ ] Uncached DMA buffer performance; consider cached + cache
      maintenance if throughput matters

## Phase 6: WiFi Driver (CYW43455) — 🟡 Skeleton Only

Skeleton compiles; every functional path is a TODO in
`drivers/wifi.rs`. Large effort — see the complexity comparison in
rpi4-networking.md before investing here.

- [ ] SDIO clock divider calculation (`wifi.rs` ~line 265 TODO)
- [ ] Firmware blob loading: `brcmfmac43455-sdio.bin` / `.txt` /
      `.clm_blob` (~line 329) — needs a storage story (embed vs SD card)
- [ ] BCDC protocol (control + data channels)
- [ ] Scan / connect / disconnect commands (~lines 352–387)
- [ ] Packet TX/RX over SDIO (~lines 425–440)
- [ ] SDIO interrupt handling (~line 457)
- [ ] WPA2 supplicant (4-way handshake) — consider "open networks only"
      first, as Ultibo did
- [ ] System description: `sdio_regs` mapping is commented out in
      `tvdemo-network.system`, and the network PD has **no GPIO mapping**
      for WL_ON (see NOTEs in `main.rs` ~lines 75–80) — required before
      the driver can even power the chip

## Phase 7: Formal Verification — ❌ Not Started

The repo's pattern: `rpi4-input-protocol` is Verus-verified; the network
ring protocol is not.

- [ ] Verus verification of `rpi4-network-protocol` ring invariants
      (index bounds, single-producer/single-consumer ownership, no
      entry reuse before release) — mirror `rpi4-input-protocol`
- [ ] Packet parser verification (ARP today; IP/TCP headers once the
      stack lands)
- [ ] Document the security argument for the Network PD's isolation
      boundary in `docs/device-isolation.md` style

## Phase 8: Hardening & Extensions — Backlog

- [ ] Multi-client support (per-client rings or a mux PD; today the
      protocol assumes exactly one client)
- [ ] Use or remove the unused `ring_flags::IN_USE` flag
- [ ] Request/response channel (MAC, link status, stats queries are
      defined in the protocol but only the shared header is served)
- [ ] Graphics/tvdemo UI: surface link status + IP in the About screen
- [ ] Modern (non-legacy) virtio-mmio support, `VIRTIO_NET_F_STATUS`
      link detection
- [ ] Pin the nightly toolchain (`rust-toolchain.toml` tracks moving
      `nightly`; drift has broken CI twice — see `633d433`, `dc4fa57`)
- [ ] ARM64 GitHub runners (`ubuntu-24.04-arm`) for KVM-speed QEMU tests
      if boot tests grow beyond smoke checks

## Testing Matrix

| Test | Where | Status |
|---|---|---|
| Network PD + rings + virtio round trip | CI (`qemu-netdemo`) | ✅ automated |
| tvdemo `NET_DRIVER=ethernet` image builds | local `make` | ✅ manual (not in CI) |
| Driver feature combos compile (`ethernet`/`wifi`/`both`/`virtio`) | local cargo | ✅ manual (not in CI) |
| GENET on real RPi4 | hardware | ❌ pending |
| WiFi anything | hardware | ❌ pending |
| DHCP/ICMP/TCP via slirp | CI | ❌ pending Phase 4 |

Candidate CI additions: build the tvdemo `NET_DRIVER=ethernet` image and
the driver feature-combo compile checks (both currently manual-only).
