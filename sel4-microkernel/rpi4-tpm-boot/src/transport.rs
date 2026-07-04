//! # TPM Transport Abstraction (workplan IC-3)
//!
//! The [`TpmTransport`] trait is the seam that makes the TPM stack
//! backend-portable: everything above it (`pcr`, `boot_chain`,
//! `attestation`, `rpi4-tpm-pd`'s frozen IPC surface) speaks TPM 2.0
//! command/response streams; everything below it moves bytes to a
//! device. The SLB9670/SPI driver is the first implementation; a
//! TIS/CRB MMIO implementation (NitroTPM, QEMU's swtpm) is a future
//! second.
//!
//! Scope (per review): this trait is for things that speak TPM 2.0
//! command streams. Crypto engines that don't (e.g. NXP CAAM) do NOT
//! qualify — a non-TPM backend would enter via a future higher-level
//! `AttestationBackend` (measure/seal/counter/quote), not by
//! stretching this trait.

use crate::commands::{self, QuoteResponse};
use crate::pcr::{PcrReadResult, PcrSelection};
use crate::slb9670::{Slb9670Tpm, TPM2_SU_CLEAR};
use crate::{Sha256Digest, TpmRc, TpmResult};

/// A channel that carries TPM 2.0 command streams to a TPM and returns
/// its response streams (workplan IC-3 — this signature is a fixed
/// interface contract).
pub trait TpmTransport {
    type Error;

    /// Submit a TPM 2.0 command, receive the response.
    ///
    /// `Ok(n)` means `resp[..n]` holds a complete response stream —
    /// including responses whose TPM response code is nonzero; TPM-level
    /// errors are protocol data, not transport failures. `Err` means the
    /// exchange itself failed (bus error, timeout, response bigger than
    /// `resp`).
    fn exchange(&mut self, cmd: &[u8], resp: &mut [u8]) -> Result<usize, Self::Error>;
}

// ============================================================================
// TRANSPORT-GENERIC TPM COMMAND LAYER
// ============================================================================

/// Response buffer size for [`Tpm`] — large enough for every response
/// this layer requests (the largest, a full 24-PCR read, is under 1KB).
pub const TPM_RESP_BUF_LEN: usize = 1024;

/// TPM 2.0 command interface over any [`TpmTransport`].
///
/// This is the type-system enforcement of transport-agnosticism the
/// workplan asks for: `pcr`/`boot_chain`/`attestation` logic composed
/// with `Tpm<T>` cannot name a bus.
pub struct Tpm<T> {
    transport: T,
    resp: [u8; TPM_RESP_BUF_LEN],
}

impl<T> Tpm<T>
where
    T: TpmTransport,
    T::Error: Into<TpmRc>,
{
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            resp: [0u8; TPM_RESP_BUF_LEN],
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn into_inner(self) -> T {
        self.transport
    }

    /// Exchange a command and validate the response header (size
    /// consistency + success response code).
    fn exchange_checked(&mut self, cmd: &[u8]) -> TpmResult<usize> {
        let n = self
            .transport
            .exchange(cmd, &mut self.resp)
            .map_err(Into::into)?;
        if n > self.resp.len() {
            return Err(TpmRc::Failure);
        }
        commands::check_response(&self.resp[..n])?;
        Ok(n)
    }

    /// TPM2_Startup(CLEAR).
    pub fn startup_clear(&mut self) -> TpmResult<()> {
        let cmd = commands::build_startup(TPM2_SU_CLEAR);
        self.exchange_checked(&cmd)?;
        Ok(())
    }

    /// TPM2_SelfTest.
    pub fn self_test(&mut self, full_test: bool) -> TpmResult<()> {
        let cmd = commands::build_self_test(full_test);
        self.exchange_checked(&cmd)?;
        Ok(())
    }

    /// TPM2_PCR_Extend with one SHA-256 digest.
    pub fn pcr_extend(&mut self, pcr_index: u8, digest: &Sha256Digest) -> TpmResult<()> {
        let cmd = commands::build_pcr_extend(pcr_index, digest)?;
        self.exchange_checked(&cmd)?;
        Ok(())
    }

    /// TPM2_PCR_Read of the SHA-256 bank.
    pub fn pcr_read(&mut self, selection: PcrSelection) -> TpmResult<PcrReadResult> {
        let cmd = commands::build_pcr_read(selection);
        let n = self.exchange_checked(&cmd)?;
        commands::parse_pcr_read(&self.resp[..n])
    }

    /// TPM2_GetRandom into `buf` (at most 32 bytes, the TPM's per-call
    /// limit for common parts).
    pub fn get_random(&mut self, buf: &mut [u8]) -> TpmResult<usize> {
        if buf.len() > 32 {
            return Err(TpmRc::BadParam);
        }
        let cmd = commands::build_get_random(buf.len() as u16);
        let n = self.exchange_checked(&cmd)?;
        let random = commands::parse_get_random(&self.resp[..n])?;
        if random.len() < buf.len() {
            return Err(TpmRc::Failure);
        }
        buf.copy_from_slice(&random[..buf.len()]);
        Ok(buf.len())
    }

    /// TPM2_Quote over the SHA-256 bank. Returns the raw TPMS_ATTEST and
    /// TPMT_SIGNATURE byte ranges (interpretation lives in
    /// `attestation.rs`). The borrows point into this `Tpm`'s response
    /// buffer and are valid until the next command.
    pub fn quote(
        &mut self,
        sign_handle: u32,
        qualifying_data: &[u8],
        selection: PcrSelection,
    ) -> TpmResult<QuoteResponse<'_>> {
        let mut cmd = [0u8; commands::QUOTE_CMD_MAX_LEN];
        let len = commands::build_quote(sign_handle, qualifying_data, selection, &mut cmd)?;
        let n = self.exchange_checked(&cmd[..len])?;
        commands::parse_quote(&self.resp[..n])
    }
}

// ============================================================================
// SLB 9670 / SPI: THE FIRST TRANSPORT IMPLEMENTATION
// ============================================================================

impl TpmTransport for Slb9670Tpm {
    type Error = TpmRc;

    fn exchange(&mut self, cmd: &[u8], resp: &mut [u8]) -> Result<usize, TpmRc> {
        // execute_command conflates transport failures with TPM-level
        // error responses; the trait treats the latter as data. If the
        // driver holds a complete response stream, hand it up regardless
        // of its response code.
        let len = match self.execute_command(cmd) {
            Ok(n) => n,
            Err(e) => {
                let n = self.response().len();
                if n >= commands::RESPONSE_HEADER_LEN {
                    n
                } else {
                    return Err(e);
                }
            }
        };
        if resp.len() < len {
            return Err(TpmRc::BadParam);
        }
        resp[..len].copy_from_slice(self.response());
        Ok(len)
    }
}

// ============================================================================
// MOCK TRANSPORT (host tests)
// ============================================================================

/// One expected command/response pair in a [`MockTransport`] script.
#[derive(Clone, Copy, Debug)]
pub struct MockExchange<'a> {
    /// The exact command stream the caller must submit.
    pub cmd: &'a [u8],
    /// The canned response stream to return.
    pub resp: &'a [u8],
}

/// A [`TpmTransport`] that replays canned TPM 2.0 command/response
/// pairs in order, failing on any deviation. `no_std`-clean, so PD
/// code can also use it for self-tests.
pub struct MockTransport<'a> {
    script: &'a [MockExchange<'a>],
    pos: usize,
}

impl<'a> MockTransport<'a> {
    pub fn new(script: &'a [MockExchange<'a>]) -> Self {
        Self { script, pos: 0 }
    }

    /// True when every scripted exchange has been consumed.
    pub fn finished(&self) -> bool {
        self.pos == self.script.len()
    }
}

impl TpmTransport for MockTransport<'_> {
    type Error = TpmRc;

    fn exchange(&mut self, cmd: &[u8], resp: &mut [u8]) -> Result<usize, TpmRc> {
        let expected = self.script.get(self.pos).ok_or(TpmRc::BadSequence)?;
        if cmd != expected.cmd {
            return Err(TpmRc::BadParam);
        }
        if resp.len() < expected.resp.len() {
            return Err(TpmRc::Failure);
        }
        resp[..expected.resp.len()].copy_from_slice(expected.resp);
        self.pos += 1;
        Ok(expected.resp.len())
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec::Vec;

    /// Build a canned response stream: header with the given tag/rc and
    /// the body appended, size filled in.
    fn response(tag: u16, rc: u32, body: &[u8]) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend_from_slice(&tag.to_be_bytes());
        r.extend_from_slice(&((10 + body.len()) as u32).to_be_bytes());
        r.extend_from_slice(&rc.to_be_bytes());
        r.extend_from_slice(body);
        r
    }

    #[test]
    fn pcr_extend_builds_the_correct_65_byte_command() {
        let digest = Sha256Digest::new([0xAB; 32]);
        let cmd = commands::build_pcr_extend(3, &digest).unwrap();

        let mut expected = Vec::new();
        expected.extend_from_slice(&[0x80, 0x02]); // TPM_ST_SESSIONS
        expected.extend_from_slice(&65u32.to_be_bytes());
        expected.extend_from_slice(&0x0000_0182u32.to_be_bytes()); // PCR_Extend
        expected.extend_from_slice(&3u32.to_be_bytes()); // handle = PCR 3
        expected.extend_from_slice(&9u32.to_be_bytes()); // authSize
        expected.extend_from_slice(&0x4000_0009u32.to_be_bytes()); // TPM_RS_PW
        expected.extend_from_slice(&[0, 0]); // nonce
        expected.push(0); // attrs
        expected.extend_from_slice(&[0, 0]); // hmac
        expected.extend_from_slice(&1u32.to_be_bytes()); // digest count
        expected.extend_from_slice(&0x000Bu16.to_be_bytes()); // SHA-256
        expected.extend_from_slice(&[0xAB; 32]);

        assert_eq!(cmd.as_slice(), expected.as_slice());
    }

    #[test]
    fn pcr_extend_rejects_invalid_index_before_any_exchange() {
        let digest = Sha256Digest::new([0; 32]);
        let mut tpm = Tpm::new(MockTransport::new(&[]));
        assert_eq!(tpm.pcr_extend(24, &digest), Err(TpmRc::BadParam));
        assert!(tpm.transport().finished());
    }

    #[test]
    fn pcr_extend_roundtrip_via_mock() {
        let digest = Sha256Digest::new([0xAB; 32]);
        let cmd = commands::build_pcr_extend(3, &digest).unwrap();
        let resp = response(0x8002, 0, &[0, 0, 0, 0, 0]); // parameterSize + auth ack
        let script = [MockExchange {
            cmd: &cmd,
            resp: &resp,
        }];
        let mut tpm = Tpm::new(MockTransport::new(&script));
        tpm.pcr_extend(3, &digest).unwrap();
        assert!(tpm.transport().finished());
    }

    #[test]
    fn tpm_error_response_surfaces_as_its_rc() {
        let cmd = commands::build_startup(TPM2_SU_CLEAR);
        let resp = response(0x8001, 0x101, &[]); // TPM_RC_FAILURE
        let script = [MockExchange {
            cmd: &cmd,
            resp: &resp,
        }];
        let mut tpm = Tpm::new(MockTransport::new(&script));
        assert_eq!(tpm.startup_clear(), Err(TpmRc::Failure));
    }

    #[test]
    fn wrong_command_is_rejected_by_the_mock() {
        let cmd = commands::build_startup(TPM2_SU_CLEAR);
        let resp = response(0x8001, 0, &[]);
        let script = [MockExchange {
            cmd: &cmd,
            resp: &resp,
        }];
        let mut tpm = Tpm::new(MockTransport::new(&script));
        // self_test != scripted startup
        assert_eq!(tpm.self_test(true), Err(TpmRc::BadParam));
    }

    #[test]
    fn get_random_roundtrip_via_mock() {
        let cmd = commands::build_get_random(8);
        let mut body = Vec::new();
        body.extend_from_slice(&8u16.to_be_bytes());
        body.extend_from_slice(&[0xD6; 8]);
        let resp = response(0x8001, 0, &body);
        let script = [MockExchange {
            cmd: &cmd,
            resp: &resp,
        }];
        let mut tpm = Tpm::new(MockTransport::new(&script));
        let mut buf = [0u8; 8];
        assert_eq!(tpm.get_random(&mut buf), Ok(8));
        assert_eq!(buf, [0xD6; 8]);
    }

    #[test]
    fn pcr_read_roundtrip_via_mock() {
        let selection = PcrSelection::from_bitmap((1 << 0) | (1 << 7));
        let cmd = commands::build_pcr_read(selection);

        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes()); // pcrUpdateCounter
        body.extend_from_slice(&1u32.to_be_bytes()); // selection count
        body.extend_from_slice(&0x000Bu16.to_be_bytes()); // SHA-256
        body.push(3); // sizeofSelect
        body.extend_from_slice(&[0x81, 0, 0]); // PCR 0 + PCR 7
        body.extend_from_slice(&2u32.to_be_bytes()); // digest count
        body.extend_from_slice(&32u16.to_be_bytes());
        body.extend_from_slice(&[0x11; 32]);
        body.extend_from_slice(&32u16.to_be_bytes());
        body.extend_from_slice(&[0x77; 32]);
        let resp = response(0x8001, 0, &body);

        let script = [MockExchange {
            cmd: &cmd,
            resp: &resp,
        }];
        let mut tpm = Tpm::new(MockTransport::new(&script));
        let result = tpm.pcr_read(selection).unwrap();
        let values = result.values();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], (0, Sha256Digest::new([0x11; 32])));
        assert_eq!(values[1], (7, Sha256Digest::new([0x77; 32])));
    }

    #[test]
    fn quote_roundtrip_via_mock() {
        let selection = PcrSelection::boot_pcrs();
        let nonce = [0x42u8; 8];
        let mut cmd = [0u8; commands::QUOTE_CMD_MAX_LEN];
        let cmd_len = commands::build_quote(0x8101_0002, &nonce, selection, &mut cmd).unwrap();
        assert_eq!(cmd_len, 49);

        // Fabricated attest + signature blobs: structure, not crypto.
        let attest = b"TEST-ATTEST";
        let mut sig = Vec::new();
        sig.extend_from_slice(&0x0014u16.to_be_bytes()); // TPM_ALG_RSASSA
        sig.extend_from_slice(&0x000Bu16.to_be_bytes()); // SHA-256
        sig.extend_from_slice(&4u16.to_be_bytes());
        sig.extend_from_slice(&[0x51; 4]);

        let mut params = Vec::new();
        params.extend_from_slice(&(attest.len() as u16).to_be_bytes());
        params.extend_from_slice(attest);
        params.extend_from_slice(&sig);

        let mut body = Vec::new();
        body.extend_from_slice(&(params.len() as u32).to_be_bytes());
        body.extend_from_slice(&params);
        body.extend_from_slice(&[0, 0, 0, 0, 0]); // session ack trailer
        let resp = response(0x8002, 0, &body);

        let script = [MockExchange {
            cmd: &cmd[..cmd_len],
            resp: &resp,
        }];
        let mut tpm = Tpm::new(MockTransport::new(&script));
        let quote = tpm.quote(0x8101_0002, &nonce, selection).unwrap();
        assert_eq!(quote.attest, attest);
        assert_eq!(quote.signature, sig.as_slice());
        assert!(tpm.transport().finished());
    }

    #[test]
    fn quote_nonce_too_long_is_rejected() {
        let mut cmd = [0u8; commands::QUOTE_CMD_MAX_LEN];
        let nonce = [0u8; 33];
        assert_eq!(
            commands::build_quote(1, &nonce, PcrSelection::all(), &mut cmd),
            Err(TpmRc::BadParam)
        );
    }

    #[test]
    fn truncated_and_lying_responses_are_rejected() {
        // Too short for a header.
        assert_eq!(commands::check_response(&[0x80]), Err(TpmRc::Failure));
        // Declared size disagrees with the stream length.
        let mut r = response(0x8001, 0, &[]);
        r.push(0xEE);
        assert_eq!(commands::check_response(&r), Err(TpmRc::Failure));
        // GetRandom body claiming more bytes than present.
        let mut body = Vec::new();
        body.extend_from_slice(&200u16.to_be_bytes());
        body.extend_from_slice(&[0xD6; 8]);
        let r = response(0x8001, 0, &body);
        assert_eq!(commands::parse_get_random(&r), Err(TpmRc::Failure));
    }
}
