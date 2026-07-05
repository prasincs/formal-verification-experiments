#!/usr/bin/env python3
"""Generate the committed tinystories GGUF fixture for the WP-6 llmdemo.

The canonical WP-6 checkpoint is a stories15M-class model fetched from
Hugging Face, but that host is not reachable from every build
environment (the remote CI sandbox allowlists only source forges and
package registries). This script therefore produces an equivalent,
fully reproducible stand-in with the same architecture family
(llama: RMSNorm + RoPE + GQA + SwiGLU, F32 tensors, GGUF v3, llama.cpp
metadata and tensor-name conventions):

  1. synthesize a deterministic tiny-stories corpus from templates
     (seeded PRNG, no external data),
  2. train a ~250K-parameter byte-level llama on it (torch, CPU,
     seeded, a few minutes),
  3. write `tinystories-260k-f32.gguf` with a self-contained GGUF v3
     writer (no gguf pip dependency).

Determinism note: the *committed artifact* is what CI pins by SHA-256.
Re-running this script on a different torch/BLAS build may produce a
bitwise-different (but behaviorally equivalent) model; regenerating the
fixture means re-pinning the hash in the demo test and CI.

Usage:  python3 generate_fixture.py [--iters N] [--out PATH]
"""

import argparse
import math
import random
import struct

# --------------------------------------------------------------------------
# 1. Deterministic synthetic corpus
# --------------------------------------------------------------------------

NAMES = ["Tom", "Lily", "Ben", "Mia", "Sam", "Ana", "Max", "Zoe"]
ANIMALS = ["cat", "dog", "bird", "fox", "frog", "bear", "duck", "mouse"]
PLACES = ["park", "garden", "forest", "house", "river", "hill", "farm", "beach"]
OBJECTS = ["ball", "book", "kite", "apple", "hat", "boat", "star", "flower"]
ADJS = ["big", "small", "red", "happy", "shiny", "soft", "funny", "little"]

SENTENCES = [
    "One day, {n} the {a} went to the {p}.",
    "{n} saw a {j} {o} near the {p}.",
    '"Look at the {j} {o}!" said {n}.',
    "{n} and the {a} played with the {o} all day.",
    "The {a} found a {j} {o} under a tree.",
    "{n} was very happy with the {j} {o}.",
    "Then {n} took the {o} home to the {p}.",
    "The {j} {a} ran to the {p} with {n}.",
    "{n} gave the {o} to the {a}.",
    "At the end, {n} and the {a} were happy.",
]


def make_corpus(rng: random.Random, n_stories: int) -> list[str]:
    stories = []
    for _ in range(n_stories):
        n = rng.choice(NAMES)
        a = rng.choice(ANIMALS)
        p = rng.choice(PLACES)
        lines = []
        for _ in range(rng.randint(3, 6)):
            t = rng.choice(SENTENCES)
            lines.append(
                t.format(n=n, a=a, p=p, o=rng.choice(OBJECTS), j=rng.choice(ADJS))
            )
        stories.append(" ".join(lines))
    return stories


# --------------------------------------------------------------------------
# 2. Byte-level tokenizer (ids: 0 <unk>, 1 <s>, 2 </s>, 3+i = byte i)
# --------------------------------------------------------------------------

UNK, BOS, EOS = 0, 1, 2
VOCAB_SIZE = 3 + 256


def encode(text: str) -> list[int]:
    return [3 + b for b in text.encode("utf-8")]


# --------------------------------------------------------------------------
# 3. Model (llama2.c-style: RMSNorm, adjacent-pair RoPE, GQA, SwiGLU)
# --------------------------------------------------------------------------


def build_and_train(corpus: list[str], iters: int, seed: int):
    import torch
    import torch.nn as nn
    import torch.nn.functional as F

    torch.manual_seed(seed)

    DIM, N_LAYERS, N_HEADS, N_KV_HEADS = 64, 5, 8, 4
    HIDDEN, SEQ_LEN = 176, 256
    HEAD = DIM // N_HEADS
    EPS = 1e-5
    ROPE_THETA = 10000.0

    class RMSNorm(nn.Module):
        def __init__(self, dim):
            super().__init__()
            self.weight = nn.Parameter(torch.ones(dim))

        def forward(self, x):
            return self.weight * x * torch.rsqrt(x.pow(2).mean(-1, keepdim=True) + EPS)

    # Adjacent-pair RoPE (llama2.c / GGML "NORM" style): rotate (x[2i], x[2i+1]).
    freqs = 1.0 / (ROPE_THETA ** (torch.arange(0, HEAD, 2).float() / HEAD))
    t = torch.arange(SEQ_LEN).float()
    ANG = torch.outer(t, freqs)  # (seq, HEAD/2)

    def rope(x, pos0=0):
        # x: (B, T, H, HEAD)
        b, tl, h, hs = x.shape
        ang = ANG[pos0 : pos0 + tl].view(1, tl, 1, hs // 2)
        x = x.view(b, tl, h, hs // 2, 2)
        x0, x1 = x[..., 0], x[..., 1]
        cos, sin = torch.cos(ang), torch.sin(ang)
        return torch.stack((x0 * cos - x1 * sin, x0 * sin + x1 * cos), dim=-1).view(
            b, tl, h, hs
        )

    class Block(nn.Module):
        def __init__(self):
            super().__init__()
            self.attn_norm = RMSNorm(DIM)
            self.wq = nn.Linear(DIM, N_HEADS * HEAD, bias=False)
            self.wk = nn.Linear(DIM, N_KV_HEADS * HEAD, bias=False)
            self.wv = nn.Linear(DIM, N_KV_HEADS * HEAD, bias=False)
            self.wo = nn.Linear(N_HEADS * HEAD, DIM, bias=False)
            self.ffn_norm = RMSNorm(DIM)
            self.w1 = nn.Linear(DIM, HIDDEN, bias=False)  # gate
            self.w2 = nn.Linear(HIDDEN, DIM, bias=False)  # down
            self.w3 = nn.Linear(DIM, HIDDEN, bias=False)  # up

        def forward(self, x):
            b, tl, _ = x.shape
            h = self.attn_norm(x)
            q = rope(self.wq(h).view(b, tl, N_HEADS, HEAD))
            k = rope(self.wk(h).view(b, tl, N_KV_HEADS, HEAD))
            v = self.wv(h).view(b, tl, N_KV_HEADS, HEAD)
            rep = N_HEADS // N_KV_HEADS
            k = k.repeat_interleave(rep, dim=2)
            v = v.repeat_interleave(rep, dim=2)
            q, k, v = (z.transpose(1, 2) for z in (q, k, v))
            att = F.scaled_dot_product_attention(q, k, v, is_causal=True)
            att = att.transpose(1, 2).reshape(b, tl, DIM)
            x = x + self.wo(att)
            h = self.ffn_norm(x)
            x = x + self.w2(F.silu(self.w1(h)) * self.w3(h))
            return x

    class Tiny(nn.Module):
        def __init__(self):
            super().__init__()
            self.tok = nn.Embedding(VOCAB_SIZE, DIM)
            self.blocks = nn.ModuleList(Block() for _ in range(N_LAYERS))
            self.norm = RMSNorm(DIM)

        def forward(self, idx):
            x = self.tok(idx)
            for blk in self.blocks:
                x = blk(x)
            x = self.norm(x)
            return x @ self.tok.weight.t()  # tied classifier

    model = Tiny()
    n_params = sum(p.numel() for p in model.parameters())
    print(f"parameters: {n_params}")

    # Token stream: <s> story </s> <s> story </s> ...
    stream = []
    for s in corpus:
        stream.append(BOS)
        stream.extend(encode(s))
        stream.append(EOS)
    data = torch.tensor(stream, dtype=torch.long)
    print(f"corpus tokens: {len(data)}")

    gen = torch.Generator().manual_seed(seed)
    opt = torch.optim.AdamW(model.parameters(), lr=1e-3, weight_decay=0.01)
    sched = torch.optim.lr_scheduler.CosineAnnealingLR(opt, T_max=iters)
    BATCH = 24
    model.train()
    for it in range(iters):
        ix = torch.randint(0, len(data) - SEQ_LEN - 1, (BATCH,), generator=gen)
        xb = torch.stack([data[i : i + SEQ_LEN] for i in ix])
        yb = torch.stack([data[i + 1 : i + SEQ_LEN + 1] for i in ix])
        logits = model(xb)
        loss = F.cross_entropy(logits.view(-1, VOCAB_SIZE), yb.view(-1))
        opt.zero_grad(set_to_none=True)
        loss.backward()
        opt.step()
        sched.step()
        if it % 50 == 0 or it == iters - 1:
            print(f"iter {it:5d}  loss {loss.item():.4f}", flush=True)

    cfg = dict(
        dim=DIM,
        n_layers=N_LAYERS,
        n_heads=N_HEADS,
        n_kv_heads=N_KV_HEADS,
        hidden=HIDDEN,
        seq_len=SEQ_LEN,
        head=HEAD,
        eps=EPS,
        rope_theta=ROPE_THETA,
    )
    return model, cfg


# --------------------------------------------------------------------------
# 4. Self-contained GGUF v3 writer (little-endian, alignment 32)
# --------------------------------------------------------------------------

GGUF_MAGIC = b"GGUF"
GGUF_VERSION = 3
ALIGNMENT = 32
# metadata value types
T_U8, T_I8, T_U16, T_I16, T_U32, T_I32, T_F32, T_BOOL, T_STR, T_ARR, T_U64 = range(11)
GGML_F32 = 0


def _s(b: bytes) -> bytes:
    return struct.pack("<Q", len(b)) + b


def _kv_str(key: str, val: str) -> bytes:
    return _s(key.encode()) + struct.pack("<I", T_STR) + _s(val.encode())


def _kv_u32(key: str, val: int) -> bytes:
    return _s(key.encode()) + struct.pack("<II", T_U32, val)


def _kv_f32(key: str, val: float) -> bytes:
    return _s(key.encode()) + struct.pack("<If", T_F32, val)


def _kv_arr_str(key: str, vals: list[str]) -> bytes:
    out = _s(key.encode()) + struct.pack("<IIQ", T_ARR, T_STR, len(vals))
    return out + b"".join(_s(v.encode()) for v in vals)


def _kv_arr_f32(key: str, vals: list[float]) -> bytes:
    out = _s(key.encode()) + struct.pack("<IIQ", T_ARR, T_F32, len(vals))
    return out + struct.pack(f"<{len(vals)}f", *vals)


def _kv_arr_i32(key: str, vals: list[int]) -> bytes:
    out = _s(key.encode()) + struct.pack("<IIQ", T_ARR, T_I32, len(vals))
    return out + struct.pack(f"<{len(vals)}i", *vals)


def write_gguf(path: str, model, cfg: dict) -> None:
    import torch

    sd = {k: v.detach().to(torch.float32) for k, v in model.state_dict().items()}

    def lin(name):
        return sd[name].contiguous()

    # llama.cpp tensor names. Linear.weight is (out, in) row-major, which is
    # GGUF ne=[in, out] with rows contiguous — exactly what llama.cpp expects.
    tensors: list[tuple[str, "torch.Tensor"]] = [("token_embd.weight", lin("tok.weight"))]
    for i in range(cfg["n_layers"]):
        p = f"blocks.{i}."
        g = f"blk.{i}."
        tensors += [
            (g + "attn_norm.weight", lin(p + "attn_norm.weight")),
            (g + "attn_q.weight", lin(p + "wq.weight")),
            (g + "attn_k.weight", lin(p + "wk.weight")),
            (g + "attn_v.weight", lin(p + "wv.weight")),
            (g + "attn_output.weight", lin(p + "wo.weight")),
            (g + "ffn_norm.weight", lin(p + "ffn_norm.weight")),
            (g + "ffn_gate.weight", lin(p + "w1.weight")),
            (g + "ffn_down.weight", lin(p + "w2.weight")),
            (g + "ffn_up.weight", lin(p + "w3.weight")),
        ]
    tensors += [
        ("output_norm.weight", lin("norm.weight")),
        ("output.weight", lin("tok.weight")),  # tied classifier, written explicitly
    ]

    tokens = ["<unk>", "<s>", "</s>"] + [f"<0x{b:02X}>" for b in range(256)]
    scores = [0.0] * len(tokens)
    # llama.cpp token types: 2 = unknown, 3 = control, 6 = byte
    ttypes = [2, 3, 3] + [6] * 256

    meta_entries = [
            _kv_str("general.architecture", "llama"),
            _kv_str("general.name", "tinystories-260k"),
            _kv_u32("general.alignment", ALIGNMENT),
            _kv_u32("llama.context_length", cfg["seq_len"]),
            _kv_u32("llama.embedding_length", cfg["dim"]),
            _kv_u32("llama.block_count", cfg["n_layers"]),
            _kv_u32("llama.feed_forward_length", cfg["hidden"]),
            _kv_u32("llama.attention.head_count", cfg["n_heads"]),
            _kv_u32("llama.attention.head_count_kv", cfg["n_kv_heads"]),
            _kv_u32("llama.rope.dimension_count", cfg["head"]),
            _kv_f32("llama.attention.layer_norm_rms_epsilon", cfg["eps"]),
            _kv_f32("llama.rope.freq_base", cfg["rope_theta"]),
            _kv_str("tokenizer.ggml.model", "llama"),
            _kv_arr_str("tokenizer.ggml.tokens", tokens),
            _kv_arr_f32("tokenizer.ggml.scores", scores),
            _kv_arr_i32("tokenizer.ggml.token_type", ttypes),
            _kv_u32("tokenizer.ggml.bos_token_id", BOS),
            _kv_u32("tokenizer.ggml.eos_token_id", EOS),
            _kv_u32("tokenizer.ggml.unknown_token_id", UNK),
    ]
    meta = b"".join(meta_entries)
    n_meta = len(meta_entries)

    # Tensor infos, computing aligned data offsets.
    infos = b""
    blobs = []
    offset = 0
    for name, ten in tensors:
        dims = list(ten.shape)[::-1]  # GGUF ne order: fastest-varying first
        infos += _s(name.encode())
        infos += struct.pack("<I", len(dims))
        infos += struct.pack(f"<{len(dims)}Q", *dims)
        infos += struct.pack("<I", GGML_F32)
        infos += struct.pack("<Q", offset)
        raw = ten.numpy().astype("<f4").tobytes()
        blobs.append((offset, raw))
        offset += len(raw)
        offset = (offset + ALIGNMENT - 1) // ALIGNMENT * ALIGNMENT

    header = GGUF_MAGIC + struct.pack("<IQQ", GGUF_VERSION, len(tensors), n_meta)
    pre = header + meta + infos
    pad = (-len(pre)) % ALIGNMENT

    with open(path, "wb") as f:
        f.write(pre)
        f.write(b"\x00" * pad)
        pos = 0
        for off, raw in blobs:
            assert pos == off, (pos, off)
            f.write(raw)
            pos += len(raw)
            padn = (-pos) % ALIGNMENT
            f.write(b"\x00" * padn)
            pos += padn
    print(f"wrote {path}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--iters", type=int, default=1500)
    ap.add_argument("--stories", type=int, default=6000)
    ap.add_argument("--seed", type=int, default=1337)
    ap.add_argument("--out", default="tinystories-260k-f32.gguf")
    args = ap.parse_args()

    rng = random.Random(args.seed)
    corpus = make_corpus(rng, args.stories)
    model, cfg = build_and_train(corpus, args.iters, args.seed)
    write_gguf(args.out, model, cfg)


if __name__ == "__main__":
    main()
