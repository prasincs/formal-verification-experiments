# update-capsule (WP-8)

`no_std` implementation of the signed update capsule format fixed by
[the workplan's IC-2](../docs/secure-agent-os-workplan.md#ic-2-update-capsule-format-wp-8--wp-3--wp-6)
— the trust boundary every hot code/model/config update crosses (design
doc: [Tier 2](../docs/secure-agent-os.md)). The library does exactly two
things:

1. **Parse** a capsule buffer, totally: for every input, either a
   validated header or a distinct rejection. Proven in Verus — no panic,
   no out-of-bounds read, no overflow, for all inputs (`src/header.rs`).
2. **Verify** a parsed capsule against the running system in IC-2's
   normative order, ending in a **one-shot install authorization** that
   names the payload by digest. The digest is the authority, not the
   buffer (`src/verify.rs`).

Applying capsules (stop → install → restart) is the supervisor's job
(WP-12, Wave 2). Transport and key provisioning are likewise out of
scope here.

## Wire format

Little-endian, fixed offsets; the layout *is* the canonical
serialization. Magic `"SAOC"`, format version 2, 0xC0-byte header:

```
0x00  [u8;4]  magic = "SAOC"
0x04  u32     format_version = 2
0x08  u8      payload_type    (1 pd-code, 2 model-weights, 3 config, 4 wasm-tool)
0x09  u8      target_slot     (slot PD id; 0 = whole-image)
0x0A  u16     target_platform (1 qemu-aarch64, 2 rpi4, 3 qemu-riscv64, ...)
0x0C  u32     abi_version
0x10  u64     monotonic_version (rollback epoch)
0x18  u64     payload_len
0x20  u64     load_vaddr      (0 = position-independent)
0x28  u64     entry_offset
0x30  u64     not_after       (MUST be 0 — no trusted time source exists)
0x38  u32     signer_key_id
0x3C  u32     key_epoch
0x40  [u8;32] payload_sha256
0x60  [u8;32] deps_sha256     (zero = none)
0x80  [u8;64] ed25519 signature over bytes [0x00..0x80) ++ payload
0xC0  ...     payload
```

The signature **binds** platform, slot + payload type, load address,
entry point, ABI version, dependency digest, rollback epoch, and expiry:
a validly signed artifact cannot be replayed into an unintended slot or
under an incompatible interface.

Implementation limits and local decisions (documented, not IC-changing):

- `payload_len ≤ u32::MAX − 0xC0`: the verified HACL* crypto entry
  points take `u32` lengths; larger declared lengths are rejected at
  parse time, never trusted.
- The buffer must be *exactly* `0xC0 + payload_len` bytes — trailing
  bytes are rejected (`TrailingData`), in the spirit of IC-2's
  unknown-field-smuggling rule.
- `key_epoch` must equal the pinned key's epoch exactly: older is
  revoked (`RevokedKeyEpoch`), newer is unvalidatable (`FutureKeyEpoch`).
- Non-code payload types (weights/config/wasm) must carry
  `load_vaddr == 0` and `entry_offset == 0` (reserved-must-be-zero);
  pd-code must have its entry inside the payload, so empty code
  payloads never verify.
- Anti-rollback accepts `monotonic_version ≥ counter` (equal permits
  re-install of the current version); the consumer bumps the scoped NV
  counter only after a successful install.

## Verification order (normative, IC-2)

```
parse (totality) → payload_len bounds → platform → slot+type → abi
  → signer key + epoch → not_after == 0 → deps digest
  → SHA-256(payload) ⟷ payload_sha256 (constant-time)
  → ed25519 signature → monotonic_version ⟷ scoped NV counter
  → InstallAuthorization { auth_id, payload_sha256, target_slot,
                           payload_type, slot_generation, monotonic_version }
```

Rollback state is scoped per `(target_slot, payload_type)` — a model
update must not burn the code slot's counter — via the `RollbackStore`
trait (a TPM NV counter in the real system, a mock on hosts).
`auth_id` freshness is the caller's obligation; in the real system the
verifier PD generates it and the installer rejects reuse. Authorization
authenticity comes from the kernel-enforced verifier→installer channel,
not from a signature — so the struct carries none.

The signed message is `prefix ++ payload`, which is non-contiguous on
the wire (the signature sits between them), so `verify_capsule` takes a
caller-provided `scratch` buffer of at least `0x80 + payload_len` bytes
to assemble it. The verifier PD points this at a private working
region; hosts pass a `Vec`.

## Crypto backend

SHA-256 and ed25519 are consumed from **formally verified**
implementations — libcrux's HACL* extractions (`libcrux-sha2`,
`libcrux-ed25519`), which build `no_std` on the pinned nightly (checked
in CI against `aarch64-unknown-none`). Nothing outside `src/crypto.rs`
names the backend; if libcrux ever fails to build, swap the two thin
wrappers there (e.g. for `sha2` + `ed25519-dalek` with
`default-features = false`) and update this section. Digest comparison
is constant-time (`crypto::ct_eq`).

## Verus proofs

`src/header.rs` is the crate's verified surface and is deliberately
self-contained so the harness can check it as a standalone crate root:

```bash
# with a Verus release on PATH (version matching the pinned
# verus_builtin 0.0.0-2025-12-07-0054):
verus --crate-type lib src/header.rs

# or via the repo's container harness (see verus/README.md):
cd ../../verus && ./run.sh shell
```

Proven: `parse` and the field accessors cannot panic, read out of
bounds, or overflow for any input, and every parsed scalar equals its
little-endian specification decode (`CapsuleHeader::parsed_from`).
Everything under `cargo build` compiles with specs stripped
(`verus!`-macro pattern, same as `rpi4-input-protocol`).

Status note: the proofs are written to the house style but the Verus
run itself is pending — the development sandbox for this change could
not fetch a Verus release (no toolchain download, no container
daemon). Run the command above before relying on the "proven" claims;
CI runs the same build/test/fuzz-sweep gates as the other verified
crates (which also don't run Verus in CI yet).

## Host tooling

```bash
# generate a signing key (or derive deterministically from a seed)
cargo run -p update-capsule-cli -- keygen --out-secret sk.bin --out-public pk.bin

# mint a signed capsule
cargo run -p update-capsule-cli -- sign \
    --secret sk.bin --payload code.bin --out capsule.bin \
    --payload-type 1 --slot 3 --platform 1 --abi 1 \
    --version 5 --key-id 1 --key-epoch 1 \
    --load-vaddr 0x40000000 --entry-offset 0x40

# inspect / verify
cargo run -p update-capsule-cli -- show capsule.bin
cargo run -p update-capsule-cli -- verify capsule.bin --public pk.bin
```

`verify` is a development aid that (unless overridden by flags) adopts
the header's own claims as the slot policy; a real verifier pins its
profile and never does that.

## Tests and fuzzing

```bash
cargo test --features mint          # unit + golden + corruption matrix +
                                    # pipeline semantics + malformed sweep
cargo test -p update-capsule-cli
cargo fuzz run parse  -- -max_total_time=60   # requires cargo-fuzz
cargo fuzz run verify -- -max_total_time=60
```

- `tests/vectors/` holds committed golden files minted with the RFC 8032
  TEST 1 key (a published test vector — unusable as a production
  secret). Signing is deterministic, so `golden_capsule_is_reproducible`
  fails on any accidental wire-format drift.
- `tests/corruption.rs` is the WP-8 acceptance matrix: each single-field
  corruption (magic, version, len, type, platform, slot, abi, key id,
  key epoch, expiry, deps, load addr, entry, hash, signature, rollback)
  rejected with its own distinct error — and order-evidence tests
  proving field checks fire before crypto, and signature before
  rollback.
- `tests/malformed.rs` is a deterministic CI-friendly sweep: 40k random
  inputs/mutations plus *exhaustive* single-byte corruption and
  truncation of the golden capsule — none may survive.

The `mint` feature (capsule construction + signing) exists for the CLI
and tests; **verifier PDs must not enable it** — a verifier carries no
signing code.
