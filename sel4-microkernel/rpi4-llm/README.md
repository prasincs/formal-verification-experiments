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
tiny-stories corpus and written by the same script's self-contained
GGUF v3 writer with llama.cpp metadata/tensor-name conventions.

Why not the canonical `stories15M` from Hugging Face: that host is not
reachable from the CI/build sandboxes this repo targets (source forges
and package registries only), and the workplan's alternative — LFS —
buys nothing for a 1 MiB file. The generator script keeps the artifact
reproducible-by-construction; a real stories15M GGUF drops in unchanged
(same architecture family, F32) wherever the network allows it, and the
loader/engine already validate everything they consume from it.

## Host demo

```bash
cargo run --release -p llmdemo-host -- \
    fixtures/tinystories-260k-f32.gguf --steps 64 \
    --expect d6271ec4ebcfa51bec2664c1d784fbba7911eb6cb31b4f93b1a57adf0ee968bb
```

prints the model hash, the validated config, the arena sizes, the
generated story text, and `TOKENS SHA256 <hex>` — the line CI asserts.
Regenerating the fixture means re-pinning that hash here, in
`tests/generate.rs`, in the loader's fixture test, and in the workflow.

## WP-6 status / follow-ups

Done: verified-surface loader, arena engine, tokenizer, host demo with
pinned deterministic output, malformed corpus + fuzz target, reference
cross-check, CI. Follow-ups tracked in the workplan: the QEMU `llmdemo`
PD product (embedding this stack unchanged behind a Microkit PD),
signed execution receipts + `llm-receipt-verify` (re-execution
challenge), quantized kernels (Q8_0/Q4_0), and extending Verus totality
through the container walk.
