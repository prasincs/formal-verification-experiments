#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "gguf==0.19.0",
#   "torch>=2.4,<3",
# ]
# ///
"""Generate the committed tinystories GGUF fixture for the WP-6 llmdemo.

The canonical WP-6 follow-up checkpoint should be a pinned
stories15M-class model, but this committed fixture is intentionally
small enough to keep in git and boot quickly under QEMU. It has the same
architecture family (llama: RMSNorm + RoPE + GQA + SwiGLU, F32 tensors,
GGUF v3, llama.cpp metadata and tensor-name conventions):

  1. synthesize a deterministic tiny-stories corpus from templates
     (seeded PRNG, no external data),
  2. train a ~250K-parameter byte-level llama on it (torch, CPU,
     seeded, a few minutes),
  3. write `tinystories-260k-f32.gguf` with llama.cpp's `gguf` Python
     writer.

Determinism note: the *committed artifact* is what CI pins by SHA-256.
Re-running this script on a different torch/BLAS build may produce a
bitwise-different (but behaviorally equivalent) model; regenerating the
fixture means re-pinning the hash in the demo test and CI.

Usage:  uv run --script generate_fixture.py [--iters N] [--out PATH]
"""

import argparse
import random

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
# 4. GGUF v3 writer (little-endian, alignment 32)
# --------------------------------------------------------------------------


def write_gguf(path: str, model, cfg: dict) -> None:
    import gguf
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

    writer = gguf.GGUFWriter(path, "llama")
    writer.add_name("tinystories-260k")
    writer.add_custom_alignment(32)
    writer.add_uint32("llama.context_length", cfg["seq_len"])
    writer.add_embedding_length(cfg["dim"])
    writer.add_block_count(cfg["n_layers"])
    writer.add_feed_forward_length(cfg["hidden"])
    writer.add_head_count(cfg["n_heads"])
    writer.add_head_count_kv(cfg["n_kv_heads"])
    writer.add_rope_dimension_count(cfg["head"])
    writer.add_layer_norm_rms_eps(cfg["eps"])
    writer.add_rope_freq_base(cfg["rope_theta"])
    writer.add_tokenizer_model("llama")
    writer.add_token_list(tokens)
    writer.add_token_scores(scores)
    writer.add_token_types(ttypes)
    writer.add_bos_token_id(BOS)
    writer.add_eos_token_id(EOS)
    writer.add_unk_token_id(UNK)

    for name, ten in tensors:
        writer.add_tensor(name, ten.numpy().astype("<f4", copy=False))

    writer.write_header_to_file()
    writer.write_kv_data_to_file()
    writer.write_tensors_to_file()
    writer.close()
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
