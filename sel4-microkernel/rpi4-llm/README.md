# rpi4-llm (WP-6)

Arena-based `no_std` llama inference engine — llama2.c's structure
(RMSNorm → RoPE attention with KV cache and GQA → SwiGLU) over weights
validated by [`rpi4-llm-loader`](../rpi4-llm-loader/). Together with the
host demo this is Route B's core: *load a small GGUF model and generate
tokens, deterministically, with no allocator and no trusted lengths.*

## Allocation discipline

No heap. [`ArenaPlan::for_model`] computes, with checked arithmetic and
only from *validated* descriptor fields, the exact sizes of two
caller-provided arenas:

- a float arena (activations, attention scores, logits, KV cache),
- a vocabulary index arena (`VocabEntry` per token).

A PD backs both with static memory sized for its model class; the host
demo uses `Vec`s. Undersized arenas fail closed
(`ArenaTooSmall`/`VocabArenaTooSmall`) — the
`docs/decoder-allocation-security.md` argument, extended to inference.

## Determinism envelope

Single-threaded, fixed evaluation order, no SIMD intrinsics or FMA
contraction, greedy argmax (lowest index wins), transcendentals from an
**exactly pinned** pure-Rust `libm`. Determinism is a *tested property*,
per the design doc: the tests and CI assert a pinned SHA-256 over the
generated token-id stream, and debug/release builds produce identical
bits. Numerics are cross-checked token-for-token against an independent
reference implementation (`fixtures/reference_infer.py`, numpy).

Engine limits (documented): F32 tensors only (quantized kernels are
follow-up), greedy sampling, SentencePiece-style greedy-BPE tokenizer
with `<0xXX>` byte fallback — whatever the checkpoint carries.

## The committed fixture

`fixtures/tinystories-260k-f32.gguf` (~1 MiB,
SHA-256 `c0d530a1…f50ad`) is a stories260K-class llama (264,256 tensor
parameters: dim 64, 5 layers, 8 heads GQA-4, byte-level vocab 259)
trained by `fixtures/generate_fixture.py` on a deterministic synthetic
tiny-stories corpus and written with llama.cpp's `gguf` Python writer
using llama.cpp metadata/tensor-name conventions.

This fixture is intentionally small enough to commit and boot quickly in
QEMU, so it is a regression target rather than the final model-size
claim. The generator script keeps the artifact reproducible by
construction; run it with `uv run --script`. A real stories15M-class
GGUF should be pinned by SHA-256 and either fetched or committed via LFS
when selected; the loader/engine path is the same and validates the
bytes it consumes.

## Host demo

```bash
cargo run --release -p llmdemo-host -- \
    fixtures/tinystories-260k-f32.gguf --steps 64 \
    --expect 7b2b33323cba78f90b50f6ac02d980f46c7e5920f1d00ba2ef736e2fe64e6dce
```

prints the model hash, the validated config, the arena sizes, the
generated story text, and `TOKENS SHA256 <hex>` — the line CI asserts.
Regenerating the fixture means re-pinning that hash here, in
`tests/generate.rs`, in the loader's fixture test, and in the workflow.

## QEMU receipt demo

`llmdemo_pd` embeds the committed fixture in a Microkit protection
domain, generates 64 tokens with the same runner as the host demo,
prints the token-id bytes beside a canonical 128-byte receipt, and signs
that receipt with a QEMU-only test application key. Verify a serial log:

```bash
cargo run --features std --bin llm-receipt-verify -- llmdemo-boot.log
```

The verifier checks the signature, prompt/model/output digests, fixed
greedy config, and deterministic reexecution.

## WP-6 status / follow-ups

Done: verified-surface loader, arena engine, tokenizer, host demo with
pinned deterministic output, malformed corpus + fuzz target, reference
cross-check, QEMU PD product, signed receipts, host reexecution
verifier, CI.

Follow-ups are tracked in [`docs/llm-roadmap.md`](../docs/llm-roadmap.md):
pinning the chosen stories15M-class artifact, moving model bytes out of
the PD image, production receipt-key hierarchy, agent-core request
protocol, quantized kernels (Q8_0/Q4_0), and coordination with
networking, capsules, TPM/attestation, policy, and CI.
