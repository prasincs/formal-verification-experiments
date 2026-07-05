//! Deterministic float kernels. All transcendentals go through the pinned
//! pure-Rust `libm`; evaluation order is fixed (plain accumulation loops).

/// `out[o] = Σ_i w[o·d_in + i] · x[i]` with `w` stored as little-endian
/// f32 bytes (weights are read in place from the model buffer; GGUF only
/// guarantees `general.alignment`, so no `&[f32]` view is assumed).
pub fn matmul(out: &mut [f32], w: &[u8], x: &[f32]) {
    let d_in = x.len();
    let rows = w.as_chunks::<4>().0.chunks_exact(d_in);
    for (o, row) in out.iter_mut().zip(rows) {
        let mut sum = 0.0f32;
        for (b, xi) in row.iter().zip(x) {
            sum += f32::from_le_bytes(*b) * xi;
        }
        *o = sum;
    }
}

/// RMSNorm: `out[i] = x[i] · g[i] / sqrt(mean(x²) + eps)` with `g` as
/// little-endian f32 bytes.
pub fn rmsnorm(out: &mut [f32], x: &[f32], g: &[u8], eps: f32) {
    let mut ss = 0.0f32;
    for v in x {
        ss += v * v;
    }
    let scale = 1.0 / libm::sqrtf(ss / x.len() as f32 + eps);
    for ((o, v), b) in out.iter_mut().zip(x).zip(g.as_chunks::<4>().0) {
        *o = f32::from_le_bytes(*b) * v * scale;
    }
}

/// In-place softmax over `x`.
pub fn softmax(x: &mut [f32]) {
    let mut max = f32::NEG_INFINITY;
    for v in x.iter() {
        if *v > max {
            max = *v;
        }
    }
    let mut sum = 0.0f32;
    for v in x.iter_mut() {
        *v = libm::expf(*v - max);
        sum += *v;
    }
    for v in x.iter_mut() {
        *v /= sum;
    }
}

/// SiLU: `v · sigmoid(v)`.
pub fn silu(v: f32) -> f32 {
    v / (1.0 + libm::expf(-v))
}

/// Rotate adjacent pairs within one head (llama2.c / GGML "NORM" RoPE).
pub fn rope_rotate(head: &mut [f32], pos: u32, theta: f32) {
    let hs = head.len();
    let mut i = 0;
    while i + 1 < hs {
        let freq = 1.0 / libm::powf(theta, i as f32 / hs as f32);
        let angle = pos as f32 * freq;
        let (sin, cos) = (libm::sinf(angle), libm::cosf(angle));
        let (x0, x1) = (head[i], head[i + 1]);
        head[i] = x0 * cos - x1 * sin;
        head[i + 1] = x0 * sin + x1 * cos;
        i += 2;
    }
}

/// Greedy argmax; ties resolve to the lowest index.
pub fn argmax(x: &[f32]) -> u32 {
    let mut best = 0usize;
    let mut best_v = f32::NEG_INFINITY;
    for (i, v) in x.iter().enumerate() {
        if *v > best_v {
            best_v = *v;
            best = i;
        }
    }
    best as u32
}
