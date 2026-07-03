# Update capsule

A `no_std` parser and verification pipeline for Secure Agent OS capsule format
version 2 (IC-2 in `docs/secure-agent-os-workplan.md`).

The verifier preserves the normative order:

1. parse the fixed header and prove payload bounds;
2. check platform, slot/type, ABI, signer/key epoch, and load address;
3. enforce expiry only when trusted wall time is available;
4. hash the payload and compare in constant time;
5. verify Ed25519 over `bytes[0x00..0x80] || payload`;
6. reject a version that is not newer than the caller-supplied rollback state
   for the exact `(target_slot, payload_type)` scope.

The core signature API is scatter/gather so the parser does not require an
allocation. The default `alloc-crypto` adapter uses `ed25519-dalek`; this is an
intentional feature-gated seam for replacing it with libcrux when the latter
builds on the repository's pinned no-std nightly.

## CLI

```sh
cargo run --manifest-path sel4-microkernel/update-capsule/Cargo.toml \
  --features cli --bin update-capsule-cli -- \
  keygen signing-key.hex verify-key.hex

cargo run --manifest-path sel4-microkernel/update-capsule/Cargo.toml \
  --features cli --bin update-capsule-cli -- \
  sign signing-key.hex payload.bin capsule.saoc \
  1 3 1 4 9 0x50000000 0x40 12 2
```

`not_after` is deliberately emitted as zero by this first CLI because the
system has monotonic counters but no specified trusted wall-clock source.

The deterministic golden vector uses a test-only signing seed of 32 bytes set
to `0x07`. It must never be provisioned on a device.

## Fuzzing

```sh
cargo fuzz run --manifest-path sel4-microkernel/update-capsule/fuzz/Cargo.toml \
  parse -- -max_total_time=60
```
