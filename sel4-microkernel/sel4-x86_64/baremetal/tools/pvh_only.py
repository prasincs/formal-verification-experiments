#!/usr/bin/env python3
"""Make a PVH-only copy of an ELF32 seL4 kernel.

    pvh_only.py IN.elf OUT.elf      # prints the PVH entry point

QEMU's -kernel loader prefers a multiboot1 header over the PVH ELF note, so
it never takes the PVH path for an image that has both. This checks that the
image has a XEN_ELFNOTE_PHYS32_ENTRY note, then zeroes the multiboot1 magic
inside QEMU's search window (first 8 KiB, 4-byte aligned). Firecracker and
Cloud Hypervisor ignore multiboot, so the copy works there too. The
multiboot2 header (GRUB) is left alone.
"""
import struct
import sys

MB1_MAGIC = 0x1BADB002
MB1_SEARCH = 8192
XEN_ELFNOTE_PHYS32_ENTRY = 18
PT_NOTE = 4


def pvh_entry(elf):
    """Entry address from the PVH note of an ELF32 little-endian image."""
    if elf[:4] != b"\x7fELF" or elf[4] != 1 or elf[5] != 1:
        raise ValueError("not an ELF32 little-endian image (objcopy -O elf32-i386 first)")
    phoff, = struct.unpack_from("<I", elf, 28)
    phentsize, phnum = struct.unpack_from("<HH", elf, 42)
    for i in range(phnum):
        p_type, p_offset, _, _, p_filesz = struct.unpack_from("<5I", elf, phoff + i * phentsize)
        if p_type != PT_NOTE:
            continue
        off, end = p_offset, p_offset + p_filesz
        while off + 12 <= end:
            namesz, descsz, ntype = struct.unpack_from("<3I", elf, off)
            name = elf[off + 12: off + 12 + namesz]
            desc = off + 12 + ((namesz + 3) & ~3)
            if name == b"Xen\0" and ntype == XEN_ELFNOTE_PHYS32_ENTRY and descsz >= 4:
                return struct.unpack_from("<I", elf, desc)[0]
            off = desc + ((descsz + 3) & ~3)
    raise ValueError("no XEN_ELFNOTE_PHYS32_ENTRY note")


def mb1_offsets(elf):
    """Offsets of valid multiboot1 headers (magic + flags + checksum == 0)."""
    out = []
    for off in range(0, min(MB1_SEARCH, len(elf) - 11), 4):
        magic, flags, cksum = struct.unpack_from("<3I", elf, off)
        if magic == MB1_MAGIC and (magic + flags + cksum) & 0xFFFFFFFF == 0:
            out.append(off)
    return out


def strip_mb1(elf):
    """(PVH-only copy, pvh entry). Fails unless exactly one mb1 header exists."""
    entry = pvh_entry(elf)
    offs = mb1_offsets(elf)
    if len(offs) != 1:
        raise ValueError(f"expected one multiboot1 header in the first {MB1_SEARCH} bytes, found {len(offs)}")
    out = bytearray(elf)
    out[offs[0]:offs[0] + 4] = bytes(4)
    assert not mb1_offsets(out)
    return bytes(out), entry


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    data, entry = strip_mb1(open(sys.argv[1], "rb").read())
    open(sys.argv[2], "wb").write(data)
    print(f"pvh entry {entry:#x}")
