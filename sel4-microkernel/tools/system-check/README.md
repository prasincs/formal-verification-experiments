# Microkit system authority checker

`system-check` parses Microkit `.system` XML into an authority graph and checks
security claims supplied by an adjacent `.system.props.toml` file.

The graph covers:

- memory mappings, permissions, and whether a region has a physical address;
- channel endpoints and IDs;
- protected-procedure call authority;
- IRQ ownership; and
- nested protection-domain parent/child relationships.

Run one system:

```sh
cargo run --manifest-path sel4-microkernel/tools/system-check/Cargo.toml -- \
  sel4-microkernel/rpi4-network/netdemo.system
```

Run every checked-in system and require a sidecar for each:

```sh
cargo run --manifest-path sel4-microkernel/tools/system-check/Cargo.toml -- \
  --all sel4-microkernel
```

## Property language

Sidecars use version 1 of the TOML schema:

```toml
version = 1

[[shared_only]]
pds = ["input", "graphics"]
regions = ["input_ring"]

[[exclusive]]
region = "uart_regs"
pd = "input"

[[no_device_mmio]]
pd = "worker"

[[only_channels]]
pd = "agent_core"
peers = ["policy"]

[[no_pp_to]]
pd = "worker"
target = "keystore"

[[dma_capable]]
pd = "network"

[[restartable_ring]]
region = "work_ring"
lifecycle_pd = "supervisor"
endpoints = ["supervisor", "worker"]
```

`shared_only` is exact in both directions: the listed PDs must have exactly the
listed regions in common, and each listed region must be mapped by exactly that
PD set. `only_channels` is also exact.

A region with `phys_addr` is classified as device/physical memory. The checker
requires explicit `dma_capable` declarations when a PD maps a physical region
whose name identifies a common DMA-capable class (`dma`, `virtio`, `genet`,
`sdio`, `usb`, `ethernet`, or `pcie`). This is intentionally conservative and
keeps the DMA trust boundary visible in review.

## Scope

Microkit channels are bidirectional kernel objects. The checker verifies which
channel capabilities exist, but it cannot prove a protocol-level notification
direction such as “producer to consumer only.” Protected-procedure authority is
directional and is checked separately. Scheduling policy and runtime protocol
behavior are outside this tool's scope.
