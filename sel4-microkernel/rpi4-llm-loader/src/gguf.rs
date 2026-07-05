//! GGUF v3 parsing to a validated [`ModelDescriptor`].
//!
//! Structure mirrors `update-capsule`: a small verified surface
//! ([`crate::bounds`]) does every byte read and every size computation as a
//! *total* operation, and this module composes those primitives into the
//! (unverified, but unsafe-free and panic-avoiding) parse of the container.
//! Every input either yields a descriptor whose tensor table has checked
//! offsets, sizes, quantization types, alignment, and pairwise
//! disjointness — or a distinct [`GgufError`]. Nothing downstream ever
//! trusts a length it read from the file.
//!
//! Implementation limits (documented rejections, not parser trust):
//! caps on tensor/key/token counts and name/key lengths, metadata arrays
//! may not nest, and only GGUF version 3 little-endian is accepted.

use crate::bounds::{self, GGML_TYPE_F16, GGML_TYPE_F32, GGML_TYPE_Q4_0, GGML_TYPE_Q8_0};

// ============================================================================
// IMPLEMENTATION LIMITS (documented, all rejections are distinct errors)
// ============================================================================

/// Maximum tensors in the table (TinyLlama-class ≈ 201; stories15M ≈ 57).
pub const MAX_TENSORS: usize = 256;
/// Maximum metadata key/value pairs (llama.cpp writes ≈ 25).
pub const MAX_KV: usize = 128;
/// Maximum metadata key length in bytes.
pub const MAX_KEY: usize = 256;
/// Maximum tensor name length in bytes (llama.cpp caps at 64).
pub const MAX_NAME: usize = 64;
/// Maximum vocabulary entries.
pub const MAX_VOCAB: usize = 65536;
/// Maximum bytes in a single vocabulary piece.
pub const MAX_TOKEN_BYTES: usize = 128;
/// Maximum tensor dimensions (GGML limit).
pub const MAX_DIMS: usize = 4;

/// Model-shape sanity caps: this loader targets sub-100M-parameter models
/// (WP-6 non-goal: anything bigger). Generous enough for TinyLlama-class.
pub const MAX_EMBED_DIM: u32 = 8192;
pub const MAX_LAYERS: u32 = 64;
pub const MAX_HEADS: u32 = 64;
pub const MAX_HIDDEN_DIM: u32 = 65536;
pub const MAX_SEQ_LEN: u32 = 32768;

const GGUF_VERSION: u32 = 3;
const DEFAULT_ALIGNMENT: u32 = 32;

// GGUF metadata value type tags.
const T_U8: u32 = 0;
const T_I8: u32 = 1;
const T_U16: u32 = 2;
const T_I16: u32 = 3;
const T_U32: u32 = 4;
const T_I32: u32 = 5;
const T_F32: u32 = 6;
const T_BOOL: u32 = 7;
const T_STR: u32 = 8;
const T_ARR: u32 = 9;
const T_U64: u32 = 10;
const T_I64: u32 = 11;
const T_F64: u32 = 12;

// ============================================================================
// ERRORS
// ============================================================================

/// Rejection reasons. Parsing is total: every malformed input maps to one
/// of these, and no partial state escapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GgufError {
    /// Buffer ends before a required field.
    Truncated,
    /// Magic is not "GGUF".
    BadMagic,
    /// Container version is not 3.
    UnsupportedVersion,
    /// Declared tensor count exceeds [`MAX_TENSORS`].
    TooManyTensors,
    /// Declared metadata count exceeds [`MAX_KV`].
    TooManyKeys,
    /// A metadata key exceeds [`MAX_KEY`] bytes.
    KeyTooLong,
    /// A metadata value has an unknown type tag.
    BadValueType,
    /// A known key carries an unexpected value type.
    BadMetadataType,
    /// Metadata arrays may not contain arrays.
    NestedArray,
    /// An array's declared element count cannot fit in the buffer.
    ArrayOutOfBounds,
    /// `general.alignment` is not a power of two in `[8, 65536]`.
    BadAlignment,
    /// `general.architecture` is missing or not "llama".
    UnsupportedArchitecture,
    /// A required `llama.*` hyperparameter is missing.
    MissingHyperparameter,
    /// A hyperparameter is zero, inconsistent, or over its cap.
    BadHyperparameter,
    /// `llama.rope.dimension_count` differs from the head size (partial
    /// RoPE is not supported).
    UnsupportedRope,
    /// An f32 hyperparameter is NaN, infinite, or out of range.
    BadFloatValue,
    /// `tokenizer.ggml.tokens` is missing.
    MissingTokenizer,
    /// Vocabulary exceeds [`MAX_VOCAB`] entries.
    TooManyTokens,
    /// A vocabulary piece exceeds [`MAX_TOKEN_BYTES`].
    TokenTooLong,
    /// `tokenizer.ggml.scores` count differs from the token count.
    ScoresMismatch,
    /// A BOS/EOS token id is outside the vocabulary.
    BadSpecialToken,
    /// A tensor name exceeds [`MAX_NAME`] bytes or is empty.
    BadTensorName,
    /// Two tensors share a name.
    DuplicateTensor,
    /// A tensor has zero or more than [`MAX_DIMS`] dimensions.
    BadDimCount,
    /// A dimension is zero or the element-count product overflows.
    BadDims,
    /// A tensor's quantization type is outside the accepted closed set.
    BadTensorType,
    /// A block-quantized tensor's row length is not a whole number of
    /// blocks, or the byte size overflows.
    BadTensorSize,
    /// A tensor's data offset is not aligned to `general.alignment`.
    MisalignedTensor,
    /// A tensor's `[offset, offset+size)` leaves the data region.
    TensorOutOfBounds,
    /// Two tensors' data ranges overlap.
    OverlappingTensors,
    /// More than one alignment's worth of unclaimed bytes trails the last
    /// tensor (nothing may ride along unvalidated).
    TrailingData,
    /// A required llama tensor is missing from the table.
    MissingTensor,
    /// A llama tensor has an unexpected shape.
    BadTensorShape,
}

type Result<T> = core::result::Result<T, GgufError>;

// ============================================================================
// TOTAL CURSOR (composes only `bounds` primitives)
// ============================================================================

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Cursor { buf, pos: 0 }
    }

    fn u8(&mut self) -> Result<u8> {
        let v = bounds::try_u8(self.buf, self.pos).ok_or(GgufError::Truncated)?;
        self.pos = self.pos.checked_add(1).ok_or(GgufError::Truncated)?;
        Ok(v)
    }

    fn u32(&mut self) -> Result<u32> {
        let v = bounds::try_u32_le(self.buf, self.pos).ok_or(GgufError::Truncated)?;
        self.pos = self.pos.checked_add(4).ok_or(GgufError::Truncated)?;
        Ok(v)
    }

    fn u64(&mut self) -> Result<u64> {
        let v = bounds::try_u64_le(self.buf, self.pos).ok_or(GgufError::Truncated)?;
        self.pos = self.pos.checked_add(8).ok_or(GgufError::Truncated)?;
        Ok(v)
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// Advance over `n` bytes, totally.
    fn skip(&mut self, n: u64) -> Result<()> {
        let n: usize = n.try_into().map_err(|_| GgufError::Truncated)?;
        let end = self.pos.checked_add(n).ok_or(GgufError::Truncated)?;
        if end > self.buf.len() {
            return Err(GgufError::Truncated);
        }
        self.pos = end;
        Ok(())
    }

    /// Read a GGUF string (u64 length + bytes), rejecting lengths over
    /// `max`. Returns the byte range within the buffer.
    fn string(&mut self, max: usize, too_long: GgufError) -> Result<ByteRange> {
        let len = self.u64()?;
        if len > max as u64 {
            return Err(too_long);
        }
        let len = len as usize;
        let start = self.pos;
        bounds::try_subslice(self.buf, start, len).ok_or(GgufError::Truncated)?;
        self.pos += len; // in-bounds: try_subslice proved start + len <= buf.len()
        Ok(ByteRange {
            off: start as u64,
            len: len as u32,
        })
    }

    fn bytes(&self, r: ByteRange) -> Result<&'a [u8]> {
        bounds::try_subslice(self.buf, r.off as usize, r.len as usize).ok_or(GgufError::Truncated)
    }
}

/// A validated byte range within the model buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ByteRange {
    pub off: u64,
    pub len: u32,
}

// ============================================================================
// DESCRIPTOR TYPES
// ============================================================================

/// Llama hyperparameters, validated for internal consistency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LlamaConfig {
    pub dim: u32,
    pub hidden_dim: u32,
    pub n_layers: u32,
    pub n_heads: u32,
    pub n_kv_heads: u32,
    pub vocab_size: u32,
    pub seq_len: u32,
    pub head_size: u32,
    pub rms_eps: f32,
    pub rope_theta: f32,
    pub bos_id: u32,
    pub eos_id: u32,
}

/// One validated tensor-table entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TensorDesc {
    name: [u8; MAX_NAME],
    name_len: u8,
    /// GGML type tag; one of the four accepted types.
    pub ty: u32,
    pub n_dims: u32,
    /// GGUF `ne` order: `dims[0]` is the fastest-varying (row) extent.
    pub dims: [u64; MAX_DIMS],
    /// Byte offset within the data region (aligned, in bounds).
    pub offset: u64,
    /// Exact byte size computed from the verified size formula.
    pub size: u64,
}

impl TensorDesc {
    const EMPTY: TensorDesc = TensorDesc {
        name: [0; MAX_NAME],
        name_len: 0,
        ty: 0,
        n_dims: 0,
        dims: [0; MAX_DIMS],
        offset: 0,
        size: 0,
    };

    pub fn name(&self) -> &[u8] {
        // name_len <= MAX_NAME is enforced at construction.
        self.name.get(..self.name_len as usize).unwrap_or(&[])
    }
}

/// The tokenizer arrays, kept as validated regions of the model buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TokenizerRegions {
    /// Number of vocabulary entries (== `LlamaConfig::vocab_size`).
    pub count: u32,
    /// Region holding `count` GGUF strings (each pre-walked and length-capped).
    pub tokens: ByteRange,
    /// Region holding `count` little-endian f32 scores, if present.
    pub scores: Option<ByteRange>,
}

/// A fully validated model: every field below has been checked against the
/// buffer it was parsed from. Plain data (offsets, not borrows) so a PD can
/// keep the descriptor in private memory while the weights stay in a
/// separate mapped region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelDescriptor {
    pub config: LlamaConfig,
    pub alignment: u32,
    /// Start of the tensor data region within the buffer.
    pub data_start: u64,
    /// Length of the data region (buffer length minus `data_start`).
    pub data_len: u64,
    pub tokenizer: TokenizerRegions,
    /// `output.weight` was absent; the classifier is tied to `token_embd`.
    pub tied_output: bool,
    n_tensors: u32,
    tensors: [TensorDesc; MAX_TENSORS],
}

impl ModelDescriptor {
    pub fn tensors(&self) -> &[TensorDesc] {
        self.tensors.get(..self.n_tensors as usize).unwrap_or(&[])
    }

    /// Linear-scan lookup by exact name.
    pub fn find(&self, name: &[u8]) -> Option<&TensorDesc> {
        self.tensors().iter().find(|t| t.name() == name)
    }

    /// The tensor's data bytes within `buf` (the same buffer that was
    /// parsed). Total: a foreign buffer yields `None`, never a panic.
    pub fn tensor_data<'a>(&self, buf: &'a [u8], t: &TensorDesc) -> Option<&'a [u8]> {
        let off = self.data_start.checked_add(t.offset)?;
        bounds::try_subslice(buf, off.try_into().ok()?, t.size.try_into().ok()?)
    }

    /// Iterate the vocabulary: yields the piece bytes for ids `0..count`.
    pub fn tokens<'a>(&self, buf: &'a [u8]) -> TokenIter<'a> {
        TokenIter {
            cur: Cursor {
                buf,
                pos: self.tokenizer.tokens.off as usize,
            },
            remaining: self.tokenizer.count,
        }
    }

    /// Score for token `id`, if a scores array is present.
    pub fn token_score(&self, buf: &[u8], id: u32) -> Option<f32> {
        let r = self.tokenizer.scores?;
        if id >= self.tokenizer.count {
            return None;
        }
        let off = (r.off as usize).checked_add(4 * id as usize)?;
        Some(f32::from_bits(bounds::try_u32_le(buf, off)?))
    }
}

/// Iterator over vocabulary pieces (regions pre-walked during parse, so
/// iteration over the parsed buffer cannot fail mid-way).
pub struct TokenIter<'a> {
    cur: Cursor<'a>,
    remaining: u32,
}

impl<'a> Iterator for TokenIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let r = self
            .cur
            .string(MAX_TOKEN_BYTES, GgufError::TokenTooLong)
            .ok()?;
        self.cur.bytes(r).ok()
    }
}

// ============================================================================
// METADATA COLLECTION
// ============================================================================

#[derive(Default)]
struct Meta {
    arch_is_llama: bool,
    alignment: Option<u32>,
    dim: Option<u32>,
    hidden_dim: Option<u32>,
    n_layers: Option<u32>,
    n_heads: Option<u32>,
    n_kv_heads: Option<u32>,
    seq_len: Option<u32>,
    rope_dims: Option<u32>,
    rms_eps: Option<f32>,
    rope_theta: Option<f32>,
    bos_id: Option<u32>,
    eos_id: Option<u32>,
    tokenizer: Option<(u32, ByteRange)>,
    scores: Option<(u32, ByteRange)>,
}

/// Read a scalar metadata value as u32 (accepting the integer widths
/// llama.cpp emits), rejecting other types.
fn value_as_u32(cur: &mut Cursor, ty: u32) -> Result<u32> {
    match ty {
        T_U8 => Ok(cur.u8()? as u32),
        T_U16 => {
            let lo = cur.u8()? as u32;
            let hi = cur.u8()? as u32;
            Ok(lo | (hi << 8))
        }
        T_U32 => cur.u32(),
        T_U64 => {
            let v = cur.u64()?;
            v.try_into().map_err(|_| GgufError::BadHyperparameter)
        }
        T_I32 => {
            let v = cur.u32()? as i32;
            v.try_into().map_err(|_| GgufError::BadHyperparameter)
        }
        _ => Err(GgufError::BadMetadataType),
    }
}

fn value_as_f32(cur: &mut Cursor, ty: u32) -> Result<f32> {
    if ty != T_F32 {
        return Err(GgufError::BadMetadataType);
    }
    cur.f32()
}

/// Skip any metadata value, totally. Arrays may not nest.
fn skip_value(cur: &mut Cursor, ty: u32, depth: u32) -> Result<()> {
    match ty {
        T_U8 | T_I8 | T_BOOL => cur.skip(1),
        T_U16 | T_I16 => cur.skip(2),
        T_U32 | T_I32 | T_F32 => cur.skip(4),
        T_U64 | T_I64 | T_F64 => cur.skip(8),
        T_STR => {
            let len = cur.u64()?;
            cur.skip(len)
        }
        T_ARR => {
            if depth > 0 {
                return Err(GgufError::NestedArray);
            }
            let elem_ty = cur.u32()?;
            let count = cur.u64()?;
            skip_array_elems(cur, elem_ty, count)
        }
        _ => Err(GgufError::BadValueType),
    }
}

fn scalar_size(ty: u32) -> Option<u64> {
    match ty {
        T_U8 | T_I8 | T_BOOL => Some(1),
        T_U16 | T_I16 => Some(2),
        T_U32 | T_I32 | T_F32 => Some(4),
        T_U64 | T_I64 | T_F64 => Some(8),
        _ => None,
    }
}

fn skip_array_elems(cur: &mut Cursor, elem_ty: u32, count: u64) -> Result<()> {
    if let Some(sz) = scalar_size(elem_ty) {
        let total = bounds::try_mul_u64(count, sz).ok_or(GgufError::ArrayOutOfBounds)?;
        return cur.skip(total);
    }
    if elem_ty == T_STR {
        // Each element consumes >= 8 bytes, so a count beyond the remaining
        // buffer is rejected before the loop — iteration stays linear in
        // the input size.
        let remaining = (cur.buf.len() - cur.pos) as u64;
        if count > remaining / 8 {
            return Err(GgufError::ArrayOutOfBounds);
        }
        for _ in 0..count {
            let len = cur.u64()?;
            cur.skip(len)?;
        }
        return Ok(());
    }
    if elem_ty == T_ARR {
        return Err(GgufError::NestedArray);
    }
    Err(GgufError::BadValueType)
}

/// Walk a string array, validating every piece length, and return
/// `(count, region)` for later random-order iteration.
fn walk_token_array(cur: &mut Cursor, ty: u32) -> Result<(u32, ByteRange)> {
    if ty != T_ARR {
        return Err(GgufError::BadMetadataType);
    }
    let elem_ty = cur.u32()?;
    if elem_ty != T_STR {
        return Err(GgufError::BadMetadataType);
    }
    let count = cur.u64()?;
    if count > MAX_VOCAB as u64 {
        return Err(GgufError::TooManyTokens);
    }
    let start = cur.pos;
    for _ in 0..count {
        cur.string(MAX_TOKEN_BYTES, GgufError::TokenTooLong)?;
    }
    Ok((
        count as u32,
        ByteRange {
            off: start as u64,
            len: (cur.pos - start) as u32,
        },
    ))
}

fn walk_scores_array(cur: &mut Cursor, ty: u32) -> Result<(u32, ByteRange)> {
    if ty != T_ARR {
        return Err(GgufError::BadMetadataType);
    }
    let elem_ty = cur.u32()?;
    if elem_ty != T_F32 {
        return Err(GgufError::BadMetadataType);
    }
    let count = cur.u64()?;
    if count > MAX_VOCAB as u64 {
        return Err(GgufError::TooManyTokens);
    }
    let start = cur.pos;
    let total = bounds::try_mul_u64(count, 4).ok_or(GgufError::ArrayOutOfBounds)?;
    cur.skip(total)?;
    Ok((
        count as u32,
        ByteRange {
            off: start as u64,
            len: (cur.pos - start) as u32,
        },
    ))
}

fn read_metadata(cur: &mut Cursor, kv_count: u64) -> Result<Meta> {
    let mut m = Meta::default();
    for _ in 0..kv_count {
        let key_range = cur.string(MAX_KEY, GgufError::KeyTooLong)?;
        let key = cur.bytes(key_range)?;
        let ty = cur.u32()?;
        match key {
            b"general.architecture" => {
                if ty != T_STR {
                    return Err(GgufError::BadMetadataType);
                }
                let v = cur.string(MAX_KEY, GgufError::KeyTooLong)?;
                m.arch_is_llama = cur.bytes(v)? == b"llama";
            }
            b"general.alignment" => m.alignment = Some(value_as_u32(cur, ty)?),
            b"llama.embedding_length" => m.dim = Some(value_as_u32(cur, ty)?),
            b"llama.feed_forward_length" => m.hidden_dim = Some(value_as_u32(cur, ty)?),
            b"llama.block_count" => m.n_layers = Some(value_as_u32(cur, ty)?),
            b"llama.attention.head_count" => m.n_heads = Some(value_as_u32(cur, ty)?),
            b"llama.attention.head_count_kv" => m.n_kv_heads = Some(value_as_u32(cur, ty)?),
            b"llama.context_length" => m.seq_len = Some(value_as_u32(cur, ty)?),
            b"llama.rope.dimension_count" => m.rope_dims = Some(value_as_u32(cur, ty)?),
            b"llama.attention.layer_norm_rms_epsilon" => m.rms_eps = Some(value_as_f32(cur, ty)?),
            b"llama.rope.freq_base" => m.rope_theta = Some(value_as_f32(cur, ty)?),
            b"tokenizer.ggml.bos_token_id" => m.bos_id = Some(value_as_u32(cur, ty)?),
            b"tokenizer.ggml.eos_token_id" => m.eos_id = Some(value_as_u32(cur, ty)?),
            b"tokenizer.ggml.tokens" => m.tokenizer = Some(walk_token_array(cur, ty)?),
            b"tokenizer.ggml.scores" => m.scores = Some(walk_scores_array(cur, ty)?),
            _ => skip_value(cur, ty, 0)?,
        }
    }
    Ok(m)
}

// ============================================================================
// PARSE
// ============================================================================

/// Parse and validate a GGUF buffer into a [`ModelDescriptor`].
///
/// Totality: for every input this returns a descriptor or a distinct
/// [`GgufError`]. All byte reads and size computations go through the
/// verified [`crate::bounds`] primitives; nothing is trusted from the file
/// without a bounds check.
pub fn parse(buf: &[u8]) -> Result<ModelDescriptor> {
    let mut cur = Cursor::new(buf);

    // --- container header ---
    if buf.len() < 4 {
        return Err(GgufError::Truncated);
    }
    if !(cur.u8()? == b'G' && cur.u8()? == b'G' && cur.u8()? == b'U' && cur.u8()? == b'F') {
        return Err(GgufError::BadMagic);
    }
    if cur.u32()? != GGUF_VERSION {
        return Err(GgufError::UnsupportedVersion);
    }
    let tensor_count = cur.u64()?;
    if tensor_count > MAX_TENSORS as u64 {
        return Err(GgufError::TooManyTensors);
    }
    let kv_count = cur.u64()?;
    if kv_count > MAX_KV as u64 {
        return Err(GgufError::TooManyKeys);
    }

    // --- metadata ---
    let meta = read_metadata(&mut cur, kv_count)?;
    if !meta.arch_is_llama {
        return Err(GgufError::UnsupportedArchitecture);
    }
    let alignment = meta.alignment.unwrap_or(DEFAULT_ALIGNMENT);
    if !(8..=65536).contains(&alignment) || !alignment.is_power_of_two() {
        return Err(GgufError::BadAlignment);
    }

    let dim = meta.dim.ok_or(GgufError::MissingHyperparameter)?;
    let hidden_dim = meta.hidden_dim.ok_or(GgufError::MissingHyperparameter)?;
    let n_layers = meta.n_layers.ok_or(GgufError::MissingHyperparameter)?;
    let n_heads = meta.n_heads.ok_or(GgufError::MissingHyperparameter)?;
    let n_kv_heads = meta.n_kv_heads.unwrap_or(n_heads);
    let seq_len = meta.seq_len.ok_or(GgufError::MissingHyperparameter)?;

    if dim == 0
        || dim > MAX_EMBED_DIM
        || hidden_dim == 0
        || hidden_dim > MAX_HIDDEN_DIM
        || n_layers == 0
        || n_layers > MAX_LAYERS
        || n_heads == 0
        || n_heads > MAX_HEADS
        || n_kv_heads == 0
        || n_kv_heads > n_heads
        || seq_len == 0
        || seq_len > MAX_SEQ_LEN
        || dim % n_heads != 0
        || n_heads % n_kv_heads != 0
    {
        return Err(GgufError::BadHyperparameter);
    }
    let head_size = dim / n_heads;
    if head_size % 2 != 0 {
        return Err(GgufError::BadHyperparameter);
    }
    if let Some(rd) = meta.rope_dims {
        if rd != head_size {
            return Err(GgufError::UnsupportedRope);
        }
    }
    let rms_eps = meta.rms_eps.unwrap_or(1e-5);
    if !(rms_eps > 0.0 && rms_eps < 1.0) {
        return Err(GgufError::BadFloatValue);
    }
    let rope_theta = meta.rope_theta.unwrap_or(10000.0);
    if !(rope_theta >= 1.0 && rope_theta.is_finite()) {
        return Err(GgufError::BadFloatValue);
    }

    let (vocab, tokens_range) = meta.tokenizer.ok_or(GgufError::MissingTokenizer)?;
    if vocab < 3 {
        return Err(GgufError::BadHyperparameter);
    }
    let scores = match meta.scores {
        Some((c, r)) => {
            if c != vocab {
                return Err(GgufError::ScoresMismatch);
            }
            Some(r)
        }
        None => None,
    };
    let bos_id = meta.bos_id.unwrap_or(1);
    let eos_id = meta.eos_id.unwrap_or(2);
    if bos_id >= vocab || eos_id >= vocab {
        return Err(GgufError::BadSpecialToken);
    }

    // --- tensor infos ---
    let mut tensors = [TensorDesc::EMPTY; MAX_TENSORS];
    let n_tensors = tensor_count as usize;
    for i in 0..n_tensors {
        let name_range = cur.string(MAX_NAME, GgufError::BadTensorName)?;
        if name_range.len == 0 {
            return Err(GgufError::BadTensorName);
        }
        let name_bytes = cur.bytes(name_range)?;

        let n_dims = cur.u32()?;
        if n_dims == 0 || n_dims > MAX_DIMS as u32 {
            return Err(GgufError::BadDimCount);
        }
        let mut dims = [1u64; MAX_DIMS];
        let mut nelems: u64 = 1;
        for d in dims.iter_mut().take(n_dims as usize) {
            let v = cur.u64()?;
            if v == 0 {
                return Err(GgufError::BadDims);
            }
            *d = v;
            nelems = bounds::try_mul_u64(nelems, v).ok_or(GgufError::BadDims)?;
        }
        let ty = cur.u32()?;
        let offset = cur.u64()?;
        let size_err = if matches!(
            ty,
            GGML_TYPE_F32 | GGML_TYPE_F16 | GGML_TYPE_Q4_0 | GGML_TYPE_Q8_0
        ) {
            GgufError::BadTensorSize
        } else {
            GgufError::BadTensorType
        };
        let size = bounds::tensor_byte_size(ty, nelems).ok_or(size_err)?;
        if !bounds::is_aligned(offset, alignment as u64) {
            return Err(GgufError::MisalignedTensor);
        }

        for prev in tensors.iter().take(i) {
            if prev.name() == name_bytes {
                return Err(GgufError::DuplicateTensor);
            }
        }

        let t = &mut tensors[i];
        t.name[..name_bytes.len()].copy_from_slice(name_bytes);
        t.name_len = name_bytes.len() as u8;
        t.ty = ty;
        t.n_dims = n_dims;
        t.dims = dims;
        t.offset = offset;
        t.size = size;
    }

    // --- data region ---
    let align = alignment as u64;
    let header_end = cur.pos as u64;
    let data_start = header_end
        .checked_add(align - 1)
        .map(|v| v / align * align)
        .ok_or(GgufError::Truncated)?;
    if data_start > buf.len() as u64 {
        return Err(GgufError::Truncated);
    }
    let data_len = buf.len() as u64 - data_start;

    let mut max_end: u64 = 0;
    for t in tensors.iter().take(n_tensors) {
        if !bounds::region_fits(t.offset, t.size, data_len) {
            return Err(GgufError::TensorOutOfBounds);
        }
        // In bounds per region_fits, so this cannot overflow.
        let end = t.offset + t.size;
        if end > max_end {
            max_end = end;
        }
    }
    for i in 0..n_tensors {
        for j in 0..i {
            let (a, b) = (&tensors[i], &tensors[j]);
            let disjoint = a.offset + a.size <= b.offset || b.offset + b.size <= a.offset;
            if !disjoint {
                return Err(GgufError::OverlappingTensors);
            }
        }
    }
    if data_len - max_end >= align {
        return Err(GgufError::TrailingData);
    }

    let desc = ModelDescriptor {
        config: LlamaConfig {
            dim,
            hidden_dim,
            n_layers,
            n_heads,
            n_kv_heads,
            vocab_size: vocab,
            seq_len,
            head_size,
            rms_eps,
            rope_theta,
            bos_id,
            eos_id,
        },
        alignment,
        data_start,
        data_len,
        tokenizer: TokenizerRegions {
            count: vocab,
            tokens: tokens_range,
            scores,
        },
        tied_output: false,
        n_tensors: n_tensors as u32,
        tensors,
    };

    validate_llama_tensors(desc)
}

// ============================================================================
// LLAMA SHAPE VALIDATION
// ============================================================================

/// Format `blk.{i}.{suffix}` into a stack buffer without alloc.
fn layer_name(buf: &mut [u8; MAX_NAME], layer: u32, suffix: &[u8]) -> usize {
    let mut n = 0;
    for &b in b"blk." {
        buf[n] = b;
        n += 1;
    }
    // layer < MAX_LAYERS <= 64, so at most 2 digits.
    if layer >= 10 {
        buf[n] = b'0' + (layer / 10) as u8;
        n += 1;
    }
    buf[n] = b'0' + (layer % 10) as u8;
    n += 1;
    for &b in suffix {
        buf[n] = b;
        n += 1;
    }
    n
}

fn expect_shape(desc: &ModelDescriptor, name: &[u8], d0: u32, d1: Option<u32>) -> Result<()> {
    let t = desc.find(name).ok_or(GgufError::MissingTensor)?;
    let want_dims = if d1.is_some() { 2 } else { 1 };
    let ok = t.n_dims == want_dims
        && t.dims[0] == d0 as u64
        && d1.map(|v| t.dims[1] == v as u64).unwrap_or(true);
    if ok {
        Ok(())
    } else {
        Err(GgufError::BadTensorShape)
    }
}

fn validate_llama_tensors(mut desc: ModelDescriptor) -> Result<ModelDescriptor> {
    let c = desc.config;
    let kv_dim = c.head_size * c.n_kv_heads;

    expect_shape(&desc, b"token_embd.weight", c.dim, Some(c.vocab_size))?;
    expect_shape(&desc, b"output_norm.weight", c.dim, None)?;
    match desc.find(b"output.weight") {
        Some(_) => expect_shape(&desc, b"output.weight", c.dim, Some(c.vocab_size))?,
        None => desc.tied_output = true,
    }

    let mut name = [0u8; MAX_NAME];
    for l in 0..c.n_layers {
        let checks: [(&[u8], u32, Option<u32>); 9] = [
            (b".attn_norm.weight", c.dim, None),
            (b".attn_q.weight", c.dim, Some(c.dim)),
            (b".attn_k.weight", c.dim, Some(kv_dim)),
            (b".attn_v.weight", c.dim, Some(kv_dim)),
            (b".attn_output.weight", c.dim, Some(c.dim)),
            (b".ffn_norm.weight", c.dim, None),
            (b".ffn_gate.weight", c.dim, Some(c.hidden_dim)),
            (b".ffn_down.weight", c.hidden_dim, Some(c.dim)),
            (b".ffn_up.weight", c.dim, Some(c.hidden_dim)),
        ];
        for (suffix, d0, d1) in checks {
            let n = layer_name(&mut name, l, suffix);
            expect_shape(&desc, &name[..n], d0, d1)?;
        }
    }
    Ok(desc)
}
