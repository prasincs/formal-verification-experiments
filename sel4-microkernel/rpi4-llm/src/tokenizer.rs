//! SentencePiece-style tokenizer over the vocabulary the checkpoint
//! carries: greedy best-score BPE merging (llama2.c's algorithm) with
//! `<0xXX>` byte fallback. No allocation: the vocabulary index lives in a
//! caller-provided arena of [`VocabEntry`].

use rpi4_llm_loader::ModelDescriptor;

/// One indexed vocabulary entry: where the piece bytes live in the model
/// buffer, its merge score, and its decoded byte if it is a `<0xXX>`
/// byte-fallback token.
#[derive(Clone, Copy, Debug, Default)]
pub struct VocabEntry {
    pub off: u32,
    pub len: u8,
    /// `0..=255` for byte tokens, `-1` otherwise.
    pub byte: i16,
    pub score: f32,
}

/// The SentencePiece whitespace marker "▁" (U+2581) in UTF-8.
pub const SP_SPACE: &[u8] = &[0xE2, 0x96, 0x81];

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

/// `<0xXX>` byte-fallback piece?
fn parse_byte_token(piece: &[u8]) -> Option<u8> {
    match piece {
        [b'<', b'0', b'x', hi, lo, b'>'] => Some(hex_val(*hi)? << 4 | hex_val(*lo)?),
        _ => None,
    }
}

/// Build the vocabulary index into `entries` (which must hold at least
/// `desc.tokenizer.count` items). Token piece offsets were pre-walked by
/// the loader, so this cannot fail on a parsed buffer.
pub fn build_index(desc: &ModelDescriptor, buf: &[u8], entries: &mut [VocabEntry]) {
    // Each piece in the pre-walked region is preceded by its u64 length,
    // so piece i starts at (previous end) + 8.
    let mut pos = desc.tokenizer.tokens.off as usize;
    for (idx, piece) in desc.tokens(buf).enumerate() {
        let Some(e) = entries.get_mut(idx) else {
            return;
        };
        pos += 8;
        e.off = pos as u32;
        e.len = piece.len() as u8;
        e.byte = parse_byte_token(piece).map(i16::from).unwrap_or(-1);
        e.score = desc.token_score(buf, idx as u32).unwrap_or(0.0);
        pos += piece.len();
    }
}

/// Piece bytes for `entry` within the model buffer.
pub fn piece<'a>(buf: &'a [u8], e: &VocabEntry) -> &'a [u8] {
    buf.get(e.off as usize..e.off as usize + e.len as usize)
        .unwrap_or(&[])
}

/// Exact-match lookup of `bytes` in the vocabulary; linear scan (prompt
/// encoding is not a hot path).
fn lookup(buf: &[u8], entries: &[VocabEntry], bytes: &[u8]) -> Option<u32> {
    entries
        .iter()
        .position(|e| piece(buf, e) == bytes)
        .map(|i| i as u32)
}

/// Encode `text` llama2.c-style: BOS, optional SentencePiece dummy space
/// prefix, per-byte pieces (with space→▁ and `<0xXX>` fallback), then
/// greedy highest-score adjacent merges. Returns the token count written
/// to `out`.
pub fn encode(
    desc: &ModelDescriptor,
    buf: &[u8],
    entries: &[VocabEntry],
    text: &[u8],
    out: &mut [u32],
) -> Result<usize, super::EngineError> {
    let mut n = 0usize;
    let mut push = |id: u32, n: &mut usize| -> Result<(), super::EngineError> {
        if *n >= out.len() {
            return Err(super::EngineError::PromptTooLong);
        }
        out[*n] = id;
        *n += 1;
        Ok(())
    };

    push(desc.config.bos_id, &mut n)?;

    // SentencePiece models mark word starts with ▁ and expect a dummy
    // prefix before the first word; byte-level vocabularies have no ▁
    // piece and take the raw bytes.
    let sp = lookup(buf, entries, SP_SPACE).is_some();
    if sp && !text.is_empty() {
        let id = lookup(buf, entries, SP_SPACE).ok_or(super::EngineError::TokenizerFailure)?;
        push(id, &mut n)?;
    }

    for &b in text {
        let mapped: &[u8] = if sp && b == b' ' { SP_SPACE } else { &[b] };
        if let Some(id) = lookup(buf, entries, mapped) {
            push(id, &mut n)?;
        } else if let Some(id) = entries.iter().position(|e| e.byte == b as i16) {
            push(id as u32, &mut n)?;
        } else {
            return Err(super::EngineError::TokenizerFailure);
        }
    }

    // Greedy merge: repeatedly replace the adjacent pair whose
    // concatenation is the highest-scoring vocabulary piece.
    let mut concat = [0u8; 256];
    loop {
        let mut best: Option<(usize, u32, f32)> = None;
        let mut i = 1; // never merge across BOS
        while i + 1 < n {
            let (a, b) = (&entries[out[i] as usize], &entries[out[i + 1] as usize]);
            let total = a.len as usize + b.len as usize;
            if total <= concat.len() {
                concat[..a.len as usize].copy_from_slice(piece(buf, a));
                concat[a.len as usize..total].copy_from_slice(piece(buf, b));
                if let Some(id) = lookup(buf, entries, &concat[..total]) {
                    let score = entries[id as usize].score;
                    if best.map(|(_, _, s)| score > s).unwrap_or(true) {
                        best = Some((i, id, score));
                    }
                }
            }
            i += 1;
        }
        match best {
            Some((i, id, _)) => {
                out[i] = id;
                out.copy_within(i + 2..n, i + 1);
                n -= 1;
            }
            None => break,
        }
    }
    Ok(n)
}

/// Decoded bytes of one generated token, written to `out`; returns the
/// byte count. Byte tokens decode to their byte; ▁ decodes to a space.
pub fn decode(buf: &[u8], entries: &[VocabEntry], id: u32, out: &mut [u8; 128]) -> usize {
    let Some(e) = entries.get(id as usize) else {
        return 0;
    };
    if e.byte >= 0 {
        out[0] = e.byte as u8;
        return 1;
    }
    let p = piece(buf, e);
    let mut n = 0;
    let mut i = 0;
    while i < p.len() {
        if p[i..].starts_with(SP_SPACE) {
            out[n] = b' ';
            n += 1;
            i += SP_SPACE.len();
        } else {
            out[n] = p[i];
            n += 1;
            i += 1;
        }
    }
    n
}
