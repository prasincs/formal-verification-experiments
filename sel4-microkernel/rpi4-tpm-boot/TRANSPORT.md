# TPM transport boundary

Wave 1 moves complete TPM command/response frames behind:

```rust
pub trait TpmTransport {
    fn exchange(
        &mut self,
        cmd: &[u8],
        resp: &mut [u8],
    ) -> Result<usize, TransportError>;
}
```

`Slb9670Tpm` implements the trait using the existing SPI/TIS path. `CrbTransport`
implements the same contract over a small `CrbIo` interface and includes an
unsafe `MmioCrb` adapter for a QEMU `tpm-crb-device` window mapped exclusively
into the TPM protection domain.

`TpmClient<T>` is the canonical command layer. It constructs and validates TPM
frames without knowing whether the transport is SPI, CRB, or a host fake. The
initial command set covers:

- startup;
- PCR extend/read;
- quote;
- NV read/increment; and
- create-primary/create/load/sign/verify-signature.

The host fake records command bytes and returns deterministic TPM frames, so
unit tests verify command codes and response parsing without privileged device
access. The old `Slb9670Tpm` convenience methods remain available for source
compatibility, but new attestation work should depend on `TpmClient<T>`.

## QEMU wiring

Map the CRB register page, command buffer, and response buffer only into the TPM
PD, then construct `MmioCrb` with those virtual addresses. The command layer and
all callers remain unchanged when moving between QEMU and the Raspberry Pi SPI
module.
