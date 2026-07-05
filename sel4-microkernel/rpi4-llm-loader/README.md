# rpi4-llm-loader (WP-6)

`no_std` GGUF model loader for the inference PD — the workplan's
centerpiece crate. A model file is untrusted input ("malicious media
file wearing a lab coat", per the
[design doc](../docs/secure-agent-os.md)): llama.cpp's loader CVEs are
parsing and allocation bugs, not math bugs. This crate applies the
repo's decoder discipline
([`docs/decoder-allocation-security.md`](../docs/decoder-allocation-security.md))
to that input.

## What parsing guarantees

`parse(buf) -> Result<ModelDescriptor, GgufError>` is **total**: every
input yields either a validated descriptor or one of ~30 distinct
rejections. On acceptance:

- the **tensor table** has checked offsets, exact byte sizes computed
  by a verified formula (no overflow, block-size divisibility for
  quantized types), a closed set of quantization types (F32/F16/
  Q4_0/Q8_0), alignment to `general.alignment`, pairwise disjoint data
  ranges, and no unclaimed trailing bytes beyond alignment slack;
- the **llama hyperparameters** are present, capped, and mutually
  consistent (`dim % heads == 0`, GQA divisibility, even head size,
  full-dimension RoPE, sane float ranges);
- the **llama tensor set** is complete with expected shapes
  (`token_embd`, per-layer attention/FFN/norms, `output_norm`, and
  `output` — absent `output` means a tied classifier, recorded as
  `tied_output`);
- the **tokenizer arrays** are pre-walked (every piece length-capped,
  scores count matching, BOS/EOS in range), so downstream vocabulary
  iteration cannot fail on a parsed buffer.

The descriptor is plain data — offsets, not borrows — so a PD keeps it
in private memory while weights stay in the shared mapped region, and
sizes every arena from validated fields only (see `rpi4-llm`).

## Verified surface (Verus)

`src/bounds.rs` is the crate's Verus surface, deliberately
self-contained so the harness checks it as a standalone crate root:

```bash
# with a Verus release on PATH (version matching the pinned
# verus_builtin 0.0.0-2025-12-07-0054):
verus --crate-type lib src/bounds.rs
```

Every function in it is **total — no `requires` clauses** — so the
(unverified) container walk in `src/gguf.rs` physically cannot misuse
the primitives: a bad offset returns `None`, never a panic, an
out-of-bounds read, or a wrap. Proven postconditions tie each reader to
a little-endian specification decode and the size computation to the
specification formula. This is the same division as
`update-capsule/src/header.rs`, adapted to a variable-layout format:
the verified core is the *byte-access and arithmetic layer*; the walk
above it is unsafe-free (`#![forbid(unsafe_code)]`), composes only
those primitives, and is covered by the malformed corpus, an
every-prefix truncation sweep, a 20k-mutation deterministic mini-fuzz,
and the `cargo-fuzz` target. Extending Verus totality up through the
walk itself is tracked follow-up work, not a claim made here.

## Implementation limits (documented rejections)

GGUF v3 little-endian only; counts capped (256 tensors, 128 metadata
keys, 64Ki vocabulary, 4 dims); names ≤ 64 bytes, keys ≤ 256, vocabulary
pieces ≤ 128; metadata arrays may not nest; models capped at
TinyLlama-class shapes (see `MAX_*` in `src/gguf.rs`).

## Tests & fuzzing

```bash
cargo test                                   # unit + malformed corpus + fixture pin
cargo build --target aarch64-unknown-none    # no_std check
cargo +nightly fuzz run parse -- -max_total_time=60
```

The fixture test pins the committed tinystories model by SHA-256; see
`../rpi4-llm/fixtures/` for its reproducible provenance.
