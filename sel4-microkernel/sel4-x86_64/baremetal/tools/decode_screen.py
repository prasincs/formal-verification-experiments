#!/usr/bin/env python3
"""Decode text from a QEMU screendump (PPM, P6) of the BOOTPROF console.

Text mode (720x400 VGA): pass --ega with a 4000-byte dump of 0xB8000 instead.
Framebuffer mode: glyphs are matched exactly against the kernel's font table
(bootprof_font8x8.h), using the kernel's scale rule. Prints one line per row.
"""
import re
import sys


def load_font(header):
    rows = re.findall(r"\{\s*((?:0x[0-9A-Fa-f]{2},\s*){7}0x[0-9A-Fa-f]{2})\s*\}", open(header).read())
    if len(rows) != 0x7F - 0x20:
        sys.exit(f"{header}: expected 95 glyphs, got {len(rows)}")
    return {tuple(int(v, 16) for v in r.split(",")): chr(0x20 + i) for i, r in enumerate(rows)}


def read_ppm(path):
    data = open(path, "rb").read()
    m = re.match(rb"P6\s+(\d+)\s+(\d+)\s+(\d+)\s", data)
    if not m:
        sys.exit(f"{path}: not a binary PPM")
    w, h = int(m.group(1)), int(m.group(2))
    return w, h, data[m.end():]


def decode_fb(ppm, header):
    font = load_font(header)
    w, h, px = read_ppm(ppm)
    scale = max(1, min(4, w // (80 * 8)))  # must match bootprof_screen_map()
    cell = 8 * scale
    lit = lambda x, y: px[(y * w + x) * 3] > 0x80
    out = []
    for r in range(h // cell):
        line = []
        for c in range(w // cell):
            bits = tuple(
                sum(lit(c * cell + x * scale, r * cell + y * scale) << x for x in range(8))
                for y in range(8)
            )
            line.append(font.get(bits, "�"))
        out.append("".join(line).rstrip())
    return out


def decode_ega(raw):
    b = open(raw, "rb").read()
    return ["".join(chr(b[(r * 80 + c) * 2]) for c in range(80)).rstrip() for r in range(25)]


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--ega":
        lines = decode_ega(sys.argv[2])
    elif len(sys.argv) == 3:
        lines = decode_fb(sys.argv[1], sys.argv[2])
    else:
        sys.exit("usage: decode_screen.py SCREEN.ppm FONT.h | --ega DUMP.bin")
    print("\n".join(lines))
