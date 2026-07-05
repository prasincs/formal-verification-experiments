# Local LLM Roadmap

Tracking document for WP-6 local inference on seL4 Microkit. Companion
to [`rpi4-llm/README.md`](../rpi4-llm/README.md) for implementation
details and [`secure-agent-os-workplan.md`](secure-agent-os-workplan.md)
for work-package ownership. Check items off as they land; each open item
calls out the other roadmap or work package it must coordinate with.

This roadmap separates two uses of "small model":

- `tinystories-260k-f32.gguf` is a committed CI/QEMU regression fixture.
- a pinned stories15M-class GGUF is the next model-size target for the
  WP-6 claim.

Do not encode Hugging Face access limits as a design constraint. If a
model should come from Hugging Face or another registry, pin the exact
artifact identity and make the download/cache policy explicit.

## Status at a Glance

| Component | Status | Verified by |
|---|---|---|
| GGUF loader verified surface | Done | unit tests, Verus bounds job, fuzz job |
| Arena-based F32 engine | Done | host tests + reference cross-check |
| Host deterministic demo | Done | pinned token-id SHA-256 in CI |
| QEMU Microkit `llmdemo` PD | Done | `QEMU PD receipt` CI job |
| Canonical test receipt + host verifier | Done | QEMU serial log + deterministic reexecution |
| Fixture generator dependency management | Done | `uv run --script`, `gguf` writer |
| stories15M-class artifact | Open | needs pinned source, hash, policy |
| Model-as-data loading path | Open | needs capsule/config integration |
| Quantized kernels | Open | Q8_0 before Q4_0 |
| Production receipt key hierarchy | Open | needs TPM/attestation work |
| Agent-core request protocol | Open | needs ring/protocol coordination |

## Coordination Map

| Area | Coordinate with | Why |
|---|---|---|
| Networking and HTTPS | WP-1, WP-13, `docs/networking-roadmap.md` | Remote model calls, artifact downloads, and cloud fallback need the network stack and TLS client. The LLM PD should not grow ad hoc network authority just to fetch weights. |
| Supervisor lifecycle | WP-3 | Long-running inference needs fault handling, restart policy, and eventually cancellation. If the LLM becomes a child PD, restartable rings must be owned by the lifecycle PD. |
| System topology checks | WP-5 | Every new `.system` file needs a `.system.props.toml` sidecar. Any future model memory region, request ring, or channel must be declared there before CI can land. |
| Update/model capsules | WP-8, WP-12, WP-16 | Model weights should become measured data artifacts, not rebuilt binaries forever. Capsule metadata must carry model identity, rollback policy, and platform/model compatibility. |
| TPM and attestation | WP-7 | Production receipts need an application receipt key certified by the TPM attestation key and sealed to PCR policy. Do not sign inference receipts directly with an attestation key. |
| Agent core integration | WP-15 | Agent-core needs a typed request/response protocol, local-vs-cloud routing, timeout/cost policy, and output provenance labels. |
| Control-plane policy | WP-17 | LLM receipts prove a deterministic local run; they do not authorize model-suggested actions. Action authorization remains a separate policy engine. |
| Build and CI budget | shared CI/build files | Larger models affect QEMU RAM, boot timeout, artifact cache size, and PR runtime. Changes must keep CI evidence useful rather than merely slow. |

## Model-Size Ladder

| Target | Current answer |
|---|---|
| 260K F32 committed fixture | Landed. Fast enough for every PR and QEMU receipt CI. |
| stories15M F32 | Next realistic target. Try host first, then QEMU with a longer timeout only if the boot remains reliable. |
| 30M-100M F32 | Possible as a local host experiment, but likely too slow/heavy for routine QEMU CI while weights are embedded in the PD image. |
| 100M+ or TinyLlama-class | Do not target until quantized kernels and model-as-data loading exist. F32 embedding makes binary size, memory, and CI time the wrong shape. |
| 1B+ | Out of scope for this Microkit PD path unless the design changes substantially; use remote/cloud routing through WP-13/WP-15 instead. |

## Phase 1: Foundation - Complete

- [x] no_std GGUF v3 parser with checked reads, sizes, tensor offsets,
      and tensor-table validation.
- [x] Arena-sized F32 llama2.c-style inference engine.
- [x] Independent numpy reference implementation.
- [x] Host `llmdemo` with deterministic 64-token output hash.
- [x] QEMU `llmdemo` Microkit product and system-check sidecar.
- [x] Canonical receipt encoding, Ed25519 test signature, and host
      verifier with deterministic reexecution.
- [x] CI jobs for loader, engine, Verus, fuzz, and QEMU receipt evidence.

## Phase 2: Artifact Policy and Generator Hygiene

- [x] Use `uv run --script` dependency metadata for fixture generation.
- [x] Use llama.cpp's `gguf` Python writer instead of a private GGUF
      writer.
- [ ] Choose the stories15M-class source artifact, including license,
      upstream URL, expected GGUF metadata, and exact SHA-256.
- [ ] Decide storage policy: committed Git LFS artifact vs. pinned
      download/cache. This is a design choice, not a workaround for this
      agent's network access.
- [ ] Add a model manifest format that can describe the committed CI
      fixture and the stories15M-class artifact with the same fields:
      source, bytes hash, expected config, tokenizer family, output hash,
      and license.

Coordination: WP-8/WP-12 if the selected storage policy is "model as
capsule"; WP-13 if fetching artifacts over HTTPS becomes part of normal
operation; CI owners if cache size or runtime changes.

## Phase 3: stories15M-Class Inference

- [ ] Run host-only inference on the selected stories15M-class GGUF and
      pin a short prompt/output token-id hash.
- [ ] Compute arena sizes from `ArenaPlan` and document the memory
      delta against the 260K fixture.
- [ ] Try QEMU boot with the stories15M-class model only after host
      determinism is pinned.
- [ ] Decide whether stories15M belongs in per-PR CI, nightly CI, or a
      manually triggered workflow. Keep the 260K fixture as the cheap
      per-PR regression target unless runtime stays small.

Coordination: shared CI/build owners for timeout and cache policy;
WP-5 for any new `.system`/sidecar changes; WP-3 if the larger run
needs watchdog or restart behavior.

## Phase 4: Model Bytes as Data

The current PD embeds the committed fixture with `include_bytes!`, which
is appropriate for the first QEMU proof but not the long-term update
story.

- [ ] Add a model data-region path or capsule-installed slot so the same
      PD binary can validate different model bytes.
- [ ] Keep parsing zero-copy: the loader validates offsets and the engine
      reads weights in place from the supplied model buffer.
- [ ] Extend receipts to include the selected model manifest identity,
      not only the raw weights digest.
- [ ] Add system-check properties for any model memory region:
      exclusive ownership, expected permissions, and no device/MMIO
      authority for the inference PD unless deliberately added.

Coordination: WP-8/WP-12/WP-16 for capsule slots and rollback policy;
WP-5 for sidecar properties; WP-3 for lifecycle ownership of model
slots if the supervisor manages installs.

## Phase 5: Receipt Hardening

- [ ] Replace the QEMU-only test key with an injected application receipt
      key in test builds and a production key story for real deployments.
- [ ] Make verifier nonce input explicit rather than fixed test data.
- [ ] Include all deterministic config fields in the receipt schema:
      prompt digest, model identity, requested steps, sampling policy,
      tokenizer/config hash, output token encoding, and version.
- [ ] Add negative verifier tests: wrong nonce, wrong model, wrong
      output bytes, wrong public key, and unsupported receipt version.
- [ ] Document what receipts prove: deterministic execution of a pinned
      model/config/output, not semantic correctness and not action
      authorization.

Coordination: WP-7 for TPM certification and PCR binding; WP-17 for how
receipts feed policy/audit; WP-8 for model identity fields shared with
capsules.

## Phase 6: Agent-Core Protocol

- [ ] Define a minimal local-inference request/response ABI:
      prompt buffer, max steps, model selector, nonce, output buffer,
      status, receipt, and token-id bytes.
- [ ] Decide whether to reuse an existing ring pattern or add an
      `rpi4-llm-protocol` crate.
- [ ] Add cancellation/timeout semantics before exposing the PD to
      agent-core.
- [ ] Keep the LLM PD authority narrow: no direct tool, network,
      display, or storage authority unless a later design explicitly
      justifies it.

Coordination: WP-15 for routing and caller API; WP-3 for lifecycle and
restart; WP-4 style ring-generation rules if a new ring protocol lands;
WP-17 for policy labels attached to outputs.

## Phase 7: Quantized Kernels and Larger Models

- [ ] Implement Q8_0 first: extend loader shape/type acceptance to the
      engine, add dequant path, and cross-check against reference output.
- [ ] Implement Q4_0 after Q8_0 is stable; treat block-size alignment and
      endian handling as loader/security surface, not just math code.
- [ ] Keep F32 as the reference lane for determinism tests.
- [ ] Re-evaluate the model-size ladder after Q8_0/Q4_0 land; only then
      consider 100M+ models for QEMU evidence.

Coordination: artifact policy in Phase 2, CI budget, and receipt schema
because quantization type and dequant semantics must be part of the
configuration being attested.

## Phase 8: Verification and Hardening

- [ ] Extend Verus totality coverage beyond scalar byte readers into the
      GGUF container walk where practical.
- [ ] Add property tests for arena sizing across generated model shapes.
- [ ] Keep tokenizer behavior pinned with byte-fallback and BPE fixtures.
- [ ] Add stack/arena regression checks for the QEMU PD so future model
      bumps do not rediscover stack faults.
- [ ] Document the inference PD isolation argument in the same style as
      the decoder and device-isolation docs.

Coordination: WP-5 for topology claims, Verus/toolchain owners for proof
style, and shared CI owners for runtime.

## Testing Matrix

| Test | Where | Status |
|---|---|---|
| Loader malformed corpus | `rpi4-llm-loader` unit tests | automated |
| Loader fuzz target | `llmdemo.yml` fuzz job | automated |
| Reference cross-check | `rpi4-llm` tests | automated |
| Host deterministic output | `llmdemo-host --expect` | automated |
| QEMU PD receipt boot | `QEMU PD receipt` CI job | automated |
| Receipt verifier reexecution | `llm-receipt-verify` in CI | automated |
| stories15M host run | local/CI TBD | pending |
| stories15M QEMU run | workflow TBD | pending |
| Quantized model run | tests TBD | pending |
| Agent-core request/response | QEMU integration TBD | pending |

Candidate CI additions: a manually triggered stories15M workflow, a
model-manifest hash check, and negative receipt-verifier tests once the
receipt schema is hardened.
