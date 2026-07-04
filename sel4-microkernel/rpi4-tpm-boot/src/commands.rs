//! # TPM 2.0 Command Builders and Response Parsers
//!
//! Transport-agnostic construction of TPM 2.0 command streams and
//! parsing of response streams. Everything here operates on plain byte
//! buffers, so it works over any [`crate::transport::TpmTransport`] —
//! SPI/TIS today, TIS/CRB MMIO or a mock tomorrow.
//!
//! Parsing follows the repo's reject-never-trust discipline: every
//! length field read from a response is bounds-checked before use.
//! (Verus totality proofs for these parsers are a design-doc goal once
//! the TPM stack joins the verified set; host tests + the update-capsule
//! parser set the pattern.)

use crate::pcr::PcrReadResult;
use crate::pcr::PcrSelection;
use crate::slb9670::{
    MAX_PCR_INDEX, TPM2_ALG_SHA256, TPM2_CC_GET_RANDOM, TPM2_CC_PCR_EXTEND, TPM2_CC_PCR_READ,
    TPM2_CC_QUOTE, TPM2_CC_SELF_TEST, TPM2_CC_STARTUP, TPM2_ST_NO_SESSIONS, TPM2_ST_SESSIONS,
};
use crate::{Sha256Digest, TpmRc, TpmResult};

/// TPM_RS_PW: the built-in password authorization session handle.
pub const TPM_RS_PW: u32 = 0x4000_0009;

/// TPM_ALG_NULL signature scheme (use the key's own scheme).
pub const TPM2_ALG_NULL: u16 = 0x0010;

/// Every TPM 2.0 response starts with tag(2) + size(4) + rc(4).
pub const RESPONSE_HEADER_LEN: usize = 10;

/// Size of the empty password authorization area used by this crate.
const PW_AUTH_LEN: u32 = 9;

/// Writes the 9-byte empty password authorization area at `off`.
fn write_pw_auth(buf: &mut [u8], off: usize) {
    buf[off..off + 4].copy_from_slice(&TPM_RS_PW.to_be_bytes());
    buf[off + 4..off + 6].copy_from_slice(&0u16.to_be_bytes()); // nonce size
    buf[off + 6] = 0; // session attributes
    buf[off + 7..off + 9].copy_from_slice(&0u16.to_be_bytes()); // hmac size
}

// ============================================================================
// COMMAND BUILDERS
// ============================================================================

/// Build TPM2_Startup.
pub fn build_startup(startup_type: u16) -> [u8; 12] {
    let mut cmd = [0u8; 12];
    cmd[0..2].copy_from_slice(&TPM2_ST_NO_SESSIONS.to_be_bytes());
    cmd[2..6].copy_from_slice(&12u32.to_be_bytes());
    cmd[6..10].copy_from_slice(&TPM2_CC_STARTUP.to_be_bytes());
    cmd[10..12].copy_from_slice(&startup_type.to_be_bytes());
    cmd
}

/// Build TPM2_SelfTest.
pub fn build_self_test(full_test: bool) -> [u8; 11] {
    let mut cmd = [0u8; 11];
    cmd[0..2].copy_from_slice(&TPM2_ST_NO_SESSIONS.to_be_bytes());
    cmd[2..6].copy_from_slice(&11u32.to_be_bytes());
    cmd[6..10].copy_from_slice(&TPM2_CC_SELF_TEST.to_be_bytes());
    cmd[10] = if full_test { 1 } else { 0 };
    cmd
}

/// Exact size of a single-digest SHA-256 TPM2_PCR_Extend command:
/// header(10) + pcrHandle(4) + authSize(4) + password auth(9)
/// + digestCount(4) + hashAlg(2) + digest(32).
pub const PCR_EXTEND_CMD_LEN: usize = 65;

/// Build TPM2_PCR_Extend for one SHA-256 digest.
///
/// (Replaces an earlier 51-byte builder that wrote past its own array —
/// the command genuinely needs 65 bytes.)
pub fn build_pcr_extend(pcr_index: u8, digest: &Sha256Digest) -> TpmResult<[u8; PCR_EXTEND_CMD_LEN]> {
    if pcr_index > MAX_PCR_INDEX {
        return Err(TpmRc::BadParam);
    }

    let mut cmd = [0u8; PCR_EXTEND_CMD_LEN];
    cmd[0..2].copy_from_slice(&TPM2_ST_SESSIONS.to_be_bytes());
    cmd[2..6].copy_from_slice(&(PCR_EXTEND_CMD_LEN as u32).to_be_bytes());
    cmd[6..10].copy_from_slice(&TPM2_CC_PCR_EXTEND.to_be_bytes());
    // PCR handle (0x00000000 + index)
    cmd[10..14].copy_from_slice(&(pcr_index as u32).to_be_bytes());
    // Authorization area
    cmd[14..18].copy_from_slice(&PW_AUTH_LEN.to_be_bytes());
    write_pw_auth(&mut cmd, 18);
    // TPML_DIGEST_VALUES: one SHA-256 entry
    cmd[27..31].copy_from_slice(&1u32.to_be_bytes());
    cmd[31..33].copy_from_slice(&TPM2_ALG_SHA256.to_be_bytes());
    cmd[33..65].copy_from_slice(&digest.bytes);
    Ok(cmd)
}

/// Exact size of a single-bank SHA-256 TPM2_PCR_Read command:
/// header(10) + count(4) + hashAlg(2) + sizeofSelect(1) + select(3).
pub const PCR_READ_CMD_LEN: usize = 20;

/// Build TPM2_PCR_Read for the SHA-256 bank.
pub fn build_pcr_read(selection: PcrSelection) -> [u8; PCR_READ_CMD_LEN] {
    let mut cmd = [0u8; PCR_READ_CMD_LEN];
    cmd[0..2].copy_from_slice(&TPM2_ST_NO_SESSIONS.to_be_bytes());
    cmd[2..6].copy_from_slice(&(PCR_READ_CMD_LEN as u32).to_be_bytes());
    cmd[6..10].copy_from_slice(&TPM2_CC_PCR_READ.to_be_bytes());
    // TPML_PCR_SELECTION with one entry
    cmd[10..14].copy_from_slice(&1u32.to_be_bytes());
    cmd[14..16].copy_from_slice(&TPM2_ALG_SHA256.to_be_bytes());
    cmd[16] = 3; // sizeofSelect: 3 bytes cover PCR 0-23
    let bitmap = selection.bitmap();
    cmd[17] = (bitmap & 0xFF) as u8;
    cmd[18] = ((bitmap >> 8) & 0xFF) as u8;
    cmd[19] = ((bitmap >> 16) & 0xFF) as u8;
    cmd
}

/// Build TPM2_GetRandom.
pub fn build_get_random(bytes_requested: u16) -> [u8; 12] {
    let mut cmd = [0u8; 12];
    cmd[0..2].copy_from_slice(&TPM2_ST_NO_SESSIONS.to_be_bytes());
    cmd[2..6].copy_from_slice(&12u32.to_be_bytes());
    cmd[6..10].copy_from_slice(&TPM2_CC_GET_RANDOM.to_be_bytes());
    cmd[10..12].copy_from_slice(&bytes_requested.to_be_bytes());
    cmd
}

/// Maximum size of a TPM2_Quote command built by [`build_quote`]:
/// header(10) + signHandle(4) + authSize(4) + password auth(9)
/// + TPM2B qualifyingData(2+32) + TPMT_SIG_SCHEME null(2)
/// + TPML_PCR_SELECTION(10).
pub const QUOTE_CMD_MAX_LEN: usize = 73;

/// Maximum qualifying-data (nonce) length accepted by [`build_quote`].
pub const QUOTE_NONCE_MAX: usize = 32;

/// Build TPM2_Quote over the SHA-256 bank, signing with `sign_handle`'s
/// own scheme (TPM_ALG_NULL). Returns the command length written.
pub fn build_quote(
    sign_handle: u32,
    qualifying_data: &[u8],
    selection: PcrSelection,
    out: &mut [u8; QUOTE_CMD_MAX_LEN],
) -> TpmResult<usize> {
    if qualifying_data.len() > QUOTE_NONCE_MAX {
        return Err(TpmRc::BadParam);
    }
    let len = 41 + qualifying_data.len();

    out[0..2].copy_from_slice(&TPM2_ST_SESSIONS.to_be_bytes());
    out[2..6].copy_from_slice(&(len as u32).to_be_bytes());
    out[6..10].copy_from_slice(&TPM2_CC_QUOTE.to_be_bytes());
    out[10..14].copy_from_slice(&sign_handle.to_be_bytes());
    out[14..18].copy_from_slice(&PW_AUTH_LEN.to_be_bytes());
    write_pw_auth(out, 18);

    let mut off = 27;
    // TPM2B qualifyingData
    out[off..off + 2].copy_from_slice(&(qualifying_data.len() as u16).to_be_bytes());
    off += 2;
    out[off..off + qualifying_data.len()].copy_from_slice(qualifying_data);
    off += qualifying_data.len();
    // TPMT_SIG_SCHEME = TPM_ALG_NULL
    out[off..off + 2].copy_from_slice(&TPM2_ALG_NULL.to_be_bytes());
    off += 2;
    // TPML_PCR_SELECTION
    out[off..off + 4].copy_from_slice(&1u32.to_be_bytes());
    off += 4;
    out[off..off + 2].copy_from_slice(&TPM2_ALG_SHA256.to_be_bytes());
    off += 2;
    out[off] = 3;
    let bitmap = selection.bitmap();
    out[off + 1] = (bitmap & 0xFF) as u8;
    out[off + 2] = ((bitmap >> 8) & 0xFF) as u8;
    out[off + 3] = ((bitmap >> 16) & 0xFF) as u8;
    off += 4;

    debug_assert_eq!(off, len);
    Ok(len)
}

// ============================================================================
// RESPONSE PARSERS
// ============================================================================

/// The fixed leading fields of every TPM 2.0 response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResponseHeader {
    pub tag: u16,
    pub size: u32,
    pub rc: u32,
}

/// Parse and validate a response stream's header: the buffer must hold
/// at least the header, the declared size must equal the buffer length,
/// and the response code must be success — a nonzero code is returned
/// as its `TpmRc`.
pub fn check_response(resp: &[u8]) -> TpmResult<ResponseHeader> {
    if resp.len() < RESPONSE_HEADER_LEN {
        return Err(TpmRc::Failure);
    }
    let header = ResponseHeader {
        tag: u16::from_be_bytes([resp[0], resp[1]]),
        size: u32::from_be_bytes([resp[2], resp[3], resp[4], resp[5]]),
        rc: u32::from_be_bytes([resp[6], resp[7], resp[8], resp[9]]),
    };
    if header.size as usize != resp.len() {
        return Err(TpmRc::Failure);
    }
    if header.rc != 0 {
        return Err(TpmRc::from(header.rc));
    }
    Ok(header)
}

/// Parse a TPM2_GetRandom response; returns the random bytes.
pub fn parse_get_random(resp: &[u8]) -> TpmResult<&[u8]> {
    check_response(resp)?;
    let body = &resp[RESPONSE_HEADER_LEN..];
    if body.len() < 2 {
        return Err(TpmRc::Failure);
    }
    let n = u16::from_be_bytes([body[0], body[1]]) as usize;
    if body.len() < 2 + n {
        return Err(TpmRc::Failure);
    }
    Ok(&body[2..2 + n])
}

/// Parse a single-bank SHA-256 TPM2_PCR_Read response into
/// `(pcr index, digest)` pairs, using the selection the TPM echoes back.
pub fn parse_pcr_read(resp: &[u8]) -> TpmResult<PcrReadResult> {
    check_response(resp)?;
    let body = &resp[RESPONSE_HEADER_LEN..];

    // pcrUpdateCounter(4) + TPML_PCR_SELECTION count(4)
    if body.len() < 8 {
        return Err(TpmRc::Failure);
    }
    let selection_count = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    if selection_count != 1 {
        // This crate only ever requests the SHA-256 bank.
        return Err(TpmRc::Failure);
    }

    // TPMS_PCR_SELECTION: hashAlg(2) + sizeofSelect(1) + select bytes
    let mut off = 8;
    if body.len() < off + 3 {
        return Err(TpmRc::Failure);
    }
    let alg = u16::from_be_bytes([body[off], body[off + 1]]);
    let size_of_select = body[off + 2] as usize;
    off += 3;
    if alg != TPM2_ALG_SHA256 || size_of_select > 3 || body.len() < off + size_of_select {
        return Err(TpmRc::Failure);
    }
    let mut bitmap: u32 = 0;
    for i in 0..size_of_select {
        bitmap |= (body[off + i] as u32) << (8 * i);
    }
    let selection = PcrSelection::from_bitmap(bitmap);
    off += size_of_select;

    // TPML_DIGEST: count(4) + count * TPM2B_DIGEST
    if body.len() < off + 4 {
        return Err(TpmRc::Failure);
    }
    let digest_count =
        u32::from_be_bytes([body[off], body[off + 1], body[off + 2], body[off + 3]]) as usize;
    off += 4;
    if digest_count > selection.count() {
        return Err(TpmRc::Failure);
    }

    let mut result = PcrReadResult::new();
    let mut indices = selection.iter();
    for _ in 0..digest_count {
        if body.len() < off + 2 {
            return Err(TpmRc::Failure);
        }
        let dlen = u16::from_be_bytes([body[off], body[off + 1]]) as usize;
        off += 2;
        if dlen != 32 || body.len() < off + dlen {
            return Err(TpmRc::Failure);
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&body[off..off + 32]);
        off += 32;
        // Digests are returned in ascending PCR order of the selection.
        let index = indices.next().ok_or(TpmRc::Failure)?;
        result.add(index, Sha256Digest::new(bytes));
    }
    Ok(result)
}

/// The two variable-length pieces of a TPM2_Quote response. `attest`
/// is the raw TPMS_ATTEST (the signed structure); `signature` is the
/// raw TPMT_SIGNATURE (algorithm-tagged). Interpretation of both stays
/// in `attestation.rs`, which is transport-agnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteResponse<'a> {
    pub attest: &'a [u8],
    pub signature: &'a [u8],
}

/// Parse a TPM2_Quote response (password session).
pub fn parse_quote(resp: &[u8]) -> TpmResult<QuoteResponse<'_>> {
    check_response(resp)?;
    let body = &resp[RESPONSE_HEADER_LEN..];

    // parameterSize(4) delimits the parameter area; the session
    // acknowledgement trails it.
    if body.len() < 4 {
        return Err(TpmRc::Failure);
    }
    let param_size = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
    if body.len() < 4 + param_size {
        return Err(TpmRc::Failure);
    }
    let params = &body[4..4 + param_size];

    // TPM2B_ATTEST
    if params.len() < 2 {
        return Err(TpmRc::Failure);
    }
    let attest_len = u16::from_be_bytes([params[0], params[1]]) as usize;
    if params.len() < 2 + attest_len {
        return Err(TpmRc::Failure);
    }
    let attest = &params[2..2 + attest_len];
    // TPMT_SIGNATURE is the rest of the parameter area.
    let signature = &params[2 + attest_len..];
    if signature.len() < 2 {
        return Err(TpmRc::Failure);
    }
    Ok(QuoteResponse { attest, signature })
}
