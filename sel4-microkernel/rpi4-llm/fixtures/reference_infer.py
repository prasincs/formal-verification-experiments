#!/usr/bin/env python3
"""Reference GGUF llama inference (numpy only) for cross-checking rpi4-llm.

Independent of both the trainer and the Rust engine: reads a GGUF v3 file,
runs a float32 llama forward pass (RMSNorm, adjacent-pair RoPE, GQA,
SwiGLU), greedy-decodes, and prints the generated text. Used as the host
reference implementation the WP-6 workplan asks the engine's numerics to be
tested against.

Usage: python3 reference_infer.py MODEL.gguf [--prompt TEXT] [--steps N]
"""

import argparse
import struct

import numpy as np

T_U8, T_I8, T_U16, T_I16, T_U32, T_I32, T_F32, T_BOOL, T_STR, T_ARR, T_U64, T_I64, T_F64 = range(13)
SCALAR_FMT = {
    T_U8: "<B", T_I8: "<b", T_U16: "<H", T_I16: "<h",
    T_U32: "<I", T_I32: "<i", T_F32: "<f", T_BOOL: "<B",
    T_U64: "<Q", T_I64: "<q", T_F64: "<d",
}


def read_gguf(path):
    buf = open(path, "rb").read()
    pos = 0

    def u32():
        nonlocal pos
        (v,) = struct.unpack_from("<I", buf, pos)
        pos += 4
        return v

    def u64():
        nonlocal pos
        (v,) = struct.unpack_from("<Q", buf, pos)
        pos += 8
        return v

    def s():
        nonlocal pos
        n = u64()
        v = buf[pos : pos + n]
        pos += n
        return v.decode("utf-8", "replace")

    def value(t):
        nonlocal pos
        if t == T_STR:
            return s()
        if t == T_ARR:
            et, n = u32(), u64()
            return [value(et) for _ in range(n)]
        fmt = SCALAR_FMT[t]
        (v,) = struct.unpack_from(fmt, buf, pos)
        pos += struct.calcsize(fmt)
        return v

    assert buf[:4] == b"GGUF", "bad magic"
    pos = 4
    version = u32()
    assert version == 3, f"unsupported version {version}"
    n_tensors, n_kv = u64(), u64()
    meta = {}
    for _ in range(n_kv):
        k = s()
        t = u32()
        meta[k] = value(t)
    infos = []
    for _ in range(n_tensors):
        name = s()
        nd = u32()
        dims = [u64() for _ in range(nd)]
        ty = u32()
        off = u64()
        infos.append((name, dims, ty, off))
    align = meta.get("general.alignment", 32)
    data_start = (pos + align - 1) // align * align
    tensors = {}
    for name, dims, ty, off in infos:
        assert ty == 0, f"reference reader supports F32 only, got type {ty}"
        n = int(np.prod(dims))
        a = np.frombuffer(buf, "<f4", count=n, offset=data_start + off)
        # GGUF ne order is fastest-first; numpy shape is slowest-first.
        tensors[name] = a.reshape(dims[::-1])
    return meta, tensors


def rmsnorm(x, g, eps):
    return g * x / np.sqrt(np.mean(x * x) + eps)


def rope(v, pos, head, theta):
    out = v.copy()
    for h in range(v.shape[0]):
        for i in range(0, head, 2):
            f = 1.0 / theta ** (i / head)
            a = pos * f
            c, s_ = np.cos(a), np.sin(a)
            x0, x1 = out[h, i], out[h, i + 1]
            out[h, i] = x0 * c - x1 * s_
            out[h, i + 1] = x0 * s_ + x1 * c
    return out


def generate(meta, tensors, prompt, steps):
    dim = meta["llama.embedding_length"]
    n_layers = meta["llama.block_count"]
    n_heads = meta["llama.attention.head_count"]
    n_kv = meta["llama.attention.head_count_kv"]
    eps = meta["llama.attention.layer_norm_rms_epsilon"]
    theta = meta.get("llama.rope.freq_base", 10000.0)
    seq_len = meta["llama.context_length"]
    head = dim // n_heads
    kv_dim = n_kv * head
    tokens_list = meta["tokenizer.ggml.tokens"]
    bos = meta["tokenizer.ggml.bos_token_id"]

    byte_ids = {f"<0x{b:02X}>": b for b in range(256)}

    def tok_bytes(i):
        t = tokens_list[i]
        if t in byte_ids:
            return bytes([byte_ids[t]])
        return t.replace("▁", " ").encode()

    ids = [bos] + [3 + b for b in prompt.encode()]

    kc = np.zeros((n_layers, seq_len, kv_dim), np.float32)
    vc = np.zeros((n_layers, seq_len, kv_dim), np.float32)
    emb = tensors["token_embd.weight"]
    out_w = tensors.get("output.weight", emb)
    rep = n_heads // n_kv

    text = bytearray()
    for pos in range(min(seq_len, len(ids) + steps)):
        tok = ids[pos] if pos < len(ids) else next_id
        if pos >= len(ids):
            text += tok_bytes(tok)
        x = emb[tok].astype(np.float32).copy()
        for l in range(n_layers):
            p = f"blk.{l}."
            h = rmsnorm(x, tensors[p + "attn_norm.weight"], eps)
            q = (tensors[p + "attn_q.weight"] @ h).reshape(n_heads, head)
            k = (tensors[p + "attn_k.weight"] @ h).reshape(n_kv, head)
            v = (tensors[p + "attn_v.weight"] @ h).reshape(n_kv, head)
            q = rope(q, pos, head, theta)
            k = rope(k, pos, head, theta)
            kc[l, pos] = k.reshape(-1)
            vc[l, pos] = v.reshape(-1)
            att_out = np.zeros(dim, np.float32)
            for hh in range(n_heads):
                kv_h = hh // rep
                ks = kc[l, : pos + 1].reshape(pos + 1, n_kv, head)[:, kv_h]
                vs = vc[l, : pos + 1].reshape(pos + 1, n_kv, head)[:, kv_h]
                sc = ks @ q[hh] / np.sqrt(head)
                sc = np.exp(sc - sc.max())
                sc /= sc.sum()
                att_out[hh * head : (hh + 1) * head] = sc @ vs
            x = x + tensors[p + "attn_output.weight"] @ att_out
            h = rmsnorm(x, tensors[p + "ffn_norm.weight"], eps)
            g = tensors[p + "ffn_gate.weight"] @ h
            u = tensors[p + "ffn_up.weight"] @ h
            g = g / (1.0 + np.exp(-g)) * u  # silu(gate) * up
            x = x + tensors[p + "ffn_down.weight"] @ g
        x = rmsnorm(x, tensors["output_norm.weight"], eps)
        logits = out_w @ x
        next_id = int(np.argmax(logits))
    return bytes(text)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("model")
    ap.add_argument("--prompt", default="One day, Tom the cat")
    ap.add_argument("--steps", type=int, default=64)
    args = ap.parse_args()
    meta, tensors = read_gguf(args.model)
    n_params = sum(t.size for t in tensors.values())
    print(f"loaded: {len(tensors)} tensors, {n_params} params")
    out = generate(meta, tensors, args.prompt, args.steps)
    print(f"prompt: {args.prompt!r}")
    print(f"output: {out.decode('utf-8', 'replace')!r}")


if __name__ == "__main__":
    main()
