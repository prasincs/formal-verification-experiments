# Verified local inference demo

`llmdemo` is the Wave 1 Route-B inference boundary. It deliberately uses a tiny
16-token transition model so the complete execution can run deterministically
in QEMU and be independently replayed by a host verifier.

The security-relevant pipeline is:

1. build or receive a GGUF-v3 model buffer;
2. parse it with `rpi4-llm-loader`, which checks every range and tensor size;
3. generate 32 tokens with fixed greedy sampling and fixed arenas;
4. hash the prompt, exact model bytes, and output bytes;
5. canonically encode a versioned 128-byte receipt;
6. sign the receipt with the QEMU-only application test key; and
7. carry the output bytes beside the receipt so the host can verify the
   signature and deterministically re-execute the model.

The receipt establishes challengeable provenance for this pinned execution. It
does not prove that the generated content is useful, truthful, or policy-safe.
The production TPM-certified receipt-key hierarchy remains Wave 2.

Run the host tests:

```sh
cargo test --manifest-path sel4-microkernel/rpi4-llm-loader/Cargo.toml
cargo test --manifest-path sel4-microkernel/rpi4-llm/Cargo.toml --features std
```

Verify a QEMU serial transcript:

```sh
cargo run --manifest-path sel4-microkernel/rpi4-llm/Cargo.toml \
  --features std --bin llm-receipt-verify -- llmdemo-boot.log
```
