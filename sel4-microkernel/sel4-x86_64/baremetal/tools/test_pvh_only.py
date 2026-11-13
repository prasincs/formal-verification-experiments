#!/usr/bin/env python3
"""Unit tests for pvh_only.py: python3 -I baremetal/tools/test_pvh_only.py"""
import os
import struct
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pvh_only as po  # noqa: E402


def note(name, ntype, desc):
    pad = lambda b: b + bytes(-len(b) % 4)
    return struct.pack("<3I", len(name), len(desc), ntype) + pad(name) + pad(desc)


def elf32(notes=b"", mb1_at=None, mb1_count=1, size=0x3000):
    """Minimal ELF32 LE image: header, one PT_NOTE phdr, notes at 0x100."""
    img = bytearray(size)
    img[:16] = b"\x7fELF" + bytes([1, 1, 1]) + bytes(9)
    struct.pack_into("<I", img, 28, 52)          # e_phoff
    struct.pack_into("<HH", img, 42, 32, 1)      # e_phentsize, e_phnum
    struct.pack_into("<5I", img, 52, po.PT_NOTE, 0x100, 0, 0, len(notes))
    img[0x100:0x100 + len(notes)] = notes
    if mb1_at is not None:
        for k in range(mb1_count):
            flags = 0x10003
            struct.pack_into("<3I", img, mb1_at + 16 * k, po.MB1_MAGIC, flags, (-(po.MB1_MAGIC + flags)) & 0xFFFFFFFF)
    return bytes(img)


PVH = note(b"Xen\0", po.XEN_ELFNOTE_PHYS32_ENTRY, struct.pack("<I", 0x10032F))


class PvhEntry(unittest.TestCase):
    def test_found_after_other_notes(self):
        notes = note(b"GNU\0", 3, b"\x01" * 20) + note(b"Xen\0", 5, b"x") + PVH
        self.assertEqual(po.pvh_entry(elf32(notes)), 0x10032F)

    def test_missing_note(self):
        with self.assertRaisesRegex(ValueError, "no XEN_ELFNOTE"):
            po.pvh_entry(elf32(note(b"GNU\0", 3, b"x")))

    def test_rejects_elf64(self):
        img = bytearray(elf32(PVH))
        img[4] = 2
        with self.assertRaisesRegex(ValueError, "ELF32"):
            po.pvh_entry(bytes(img))


class StripMb1(unittest.TestCase):
    def test_strips_only_magic(self):
        img = elf32(PVH, mb1_at=0x120)
        out, entry = po.strip_mb1(img)
        self.assertEqual(entry, 0x10032F)
        self.assertEqual(out[0x120:0x124], bytes(4))
        self.assertEqual(out[0x124:], img[0x124:])   # flags/checksum and the rest untouched
        self.assertEqual(out[:0x120], img[:0x120])
        self.assertEqual(po.mb1_offsets(out), [])

    def test_ignores_magic_with_bad_checksum_or_outside_window(self):
        img = bytearray(elf32(PVH, mb1_at=0x120))
        struct.pack_into("<I", img, 0x200, po.MB1_MAGIC)  # no valid checksum
        struct.pack_into("<3I", img, po.MB1_SEARCH + 4, po.MB1_MAGIC, 0, (-po.MB1_MAGIC) & 0xFFFFFFFF)
        self.assertEqual(po.mb1_offsets(bytes(img)), [0x120])

    def test_requires_exactly_one_header(self):
        with self.assertRaisesRegex(ValueError, "found 0"):
            po.strip_mb1(elf32(PVH))
        with self.assertRaisesRegex(ValueError, "found 2"):
            po.strip_mb1(elf32(PVH, mb1_at=0x120, mb1_count=2))

    def test_requires_pvh_note(self):
        with self.assertRaises(ValueError):
            po.strip_mb1(elf32(b"", mb1_at=0x120))


if __name__ == "__main__":
    unittest.main()
