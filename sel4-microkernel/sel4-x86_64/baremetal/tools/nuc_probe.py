#!/usr/bin/env python3
"""Probe an x86_64 machine (from a Linux boot, e.g. an Ubuntu live USB) for
the facts the seL4 BOOTPROF image depends on, and print a report.

    sudo python3 nuc_probe.py            # report on stdout + ./sel4-probe.txt

Read-only: loads the msr/cpuid modules and reads MSRs, CPUID, sysfs, dmesg.
Stdlib only, so it runs on a stock live image.
"""
import glob
import os
import re
import struct
import subprocess
import sys

# Mirrors seL4 44b2b946 (include/arch/x86/arch/machine.h, tsc_init()):
# display-model IDs for which seL4 reads MSR_PLATFORM_INFO instead of
# calibrating against the PIT (~100 ms).
SEL4_TSC_MSR_MODELS = {
    0x1E, 0x1A, 0x2E,                    # Nehalem (ratio * 133.33 MHz)
    0x2A, 0x2D,                          # Sandy Bridge
    0x3C, 0x3F, 0x45, 0x46,              # Haswell
    0x9A, 0x3E, 0x3A,                    # "Ivy Bridge" (0x9A is really Alder Lake-P)
    0x56, 0x4F, 0x47, 0x3D,              # Broadwell
    0x4E, 0x5E,                          # Skylake
}
NEHALEM_MODELS = {0x1E, 0x1A, 0x2E}
# Crystal clock for CPUID.15H when ECX reports 0 (as Linux native_calibrate_tsc).
CRYSTAL_HZ = {
    **{m: 24_000_000 for m in (0x4E, 0x5E, 0x8E, 0x9E, 0xA5, 0xA6)},  # Skylake..Comet Lake client
    0x5F: 25_000_000,                                              # Denverton
    0x5C: 19_200_000,                                              # Goldmont (Apollo Lake)
}
PADDR_TOP = 0xFFFFFFFF80000000 - 0xFFFFFF8000000000  # seL4 x86_64 kernel window: 510 GiB
SEL4_LOAD_REGION = (0x100000, 0x1000000)  # kernel at 1 MiB + root-task scratch; generous
MSR_TSC, MSR_TSC_ADJUST, MSR_PLATFORM_INFO = 0x10, 0x3B, 0xCE


# ---------------------------------------------------------------- parsers --

def parse_cpuinfo(text):
    first = text.split("\n\n", 1)[0]
    kv = dict(
        (k.strip(), v.strip())
        for k, _, v in (line.partition(":") for line in first.splitlines())
        if _
    )
    return {
        "vendor": kv.get("vendor_id", ""),
        "name": kv.get("model name", ""),
        "family": int(kv.get("cpu family", "0")),
        "model": int(kv.get("model", "0")),
        "stepping": int(kv.get("stepping", "0")),
        "flags": set(kv.get("flags", "").split()),
        "cpus": text.count("\nprocessor") + text.startswith("processor"),
    }


def sel4_tsc_path(cpu):
    if cpu["vendor"] == "GenuineIntel" and cpu["family"] == 6 and cpu["model"] in SEL4_TSC_MSR_MODELS:
        return "msr"
    return "pit"


def platform_info_mhz(msr_value, model):
    ratio = (msr_value >> 8) & 0xFF
    if ratio == 0:
        return None
    return ratio * 13333 // 100 if model in NEHALEM_MODELS else ratio * 100


def cpuid15_tsc_hz(eax, ebx, ecx, model):
    """TSC Hz from CPUID.15H (denominator, numerator, crystal Hz)."""
    if eax == 0 or ebx == 0:
        return None
    crystal = ecx or CRYSTAL_HZ.get(model)
    return crystal * ebx // eax if crystal else None


def parse_dmesg_tsc_mhz(dmesg):
    """Prefer the refined calibration; fall back to the initial detection."""
    for pat in (r"Refined TSC clocksource calibration: ([\d.]+) MHz",
                r"tsc: Detected ([\d.]+) MHz processor",
                r"tsc: Detected ([\d.]+) MHz TSC"):
        m = re.findall(pat, dmesg)
        if m:
            return float(m[-1])
    return None


def parse_efifb(dmesg):
    fb = {}
    m = re.search(r"efifb: framebuffer at (0x[0-9a-fA-F]+)", dmesg)
    if m:
        fb["addr"] = int(m.group(1), 16)
    m = re.search(r"efifb: mode is (\d+)x(\d+)x(\d+), linelength=(\d+)", dmesg)
    if m:
        fb.update(width=int(m.group(1)), height=int(m.group(2)),
                  bpp=int(m.group(3)), pitch=int(m.group(4)))
    return fb or None


def parse_iomem_bootfb(iomem):
    """Boot framebuffer address from /proc/iomem (sysfb: BOOTFB, or efifb)."""
    m = re.search(r"^\s*([0-9a-f]+)-[0-9a-f]+ : (?:BOOTFB|efifb)\s*$", iomem, re.M)
    return int(m.group(1), 16) if m else None


def parse_fb0(name, virtual_size, bpp, stride):
    """/sys/class/graphics/fb0 attributes -> mode dict, for simpledrm/efifb only
    (a native driver such as i915 has already replaced the GOP mode)."""
    if not name or not re.search(r"simple|efi", name, re.I):
        return None
    w, h = (int(v) for v in virtual_size.split(","))
    return {"width": w, "height": h, "bpp": int(bpp), "pitch": int(stride)}


def parse_e820(dmesg):
    out = []
    for s, e, t in re.findall(r"BIOS-e820: \[mem (0x[0-9a-f]+)-(0x[0-9a-f]+)\] (.+)", dmesg):
        out.append((int(s, 16), int(e, 16) + 1, t.strip()))
    return out


def region_usable(regions, start, end):
    """True iff [start, end) is fully covered by 'usable' e820 ranges."""
    pos = start
    for s, e, t in sorted(regions):
        if t != "usable" or e <= pos:
            continue
        if s > pos:
            return False
        pos = e
        if pos >= end:
            return True
    return pos >= end


def mtrr_type_at(mtrr_text, addr):
    """Effective variable-MTRR type for addr (UC wins on overlap), or None."""
    types = []
    for base, size_mb, t in re.findall(r"base=(0x[0-9a-f]+) \( *\d+MB\), size= *(\d+)MB, count=\d+: (\S+)", mtrr_text):
        b = int(base, 16)
        if b <= addr < b + int(size_mb) * 2**20:
            types.append(t)
    if not types:
        return None
    return "uncachable" if "uncachable" in types else types[0]


def console_geometry(width, height):
    """Mirror of bootprof_screen_map() for an RGB framebuffer."""
    scale = max(1, min(4, width // (80 * 8)))
    return scale, width // (8 * scale), height // (8 * scale)


def parse_serial(text):
    """/proc/tty/driver/serial -> {line: (uart, port)} for detected UARTs."""
    out = {}
    for line, uart, port in re.findall(r"^(\d+): uart:(\S+) port:([0-9A-Fa-f]+)", text, re.M):
        if uart != "unknown":
            out[int(line)] = (uart, int(port, 16))
    return out


# ------------------------------------------------------------- collectors --

def read(path, binary=False):
    try:
        with open(path, "rb" if binary else "r") as f:
            return f.read()
    except OSError:
        return None


def run(*cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=20).stdout
    except (OSError, subprocess.SubprocessError):
        return ""


def rdmsr(msr, cpu=0):
    try:
        fd = os.open(f"/dev/cpu/{cpu}/msr", os.O_RDONLY)
        try:
            return struct.unpack("<Q", os.pread(fd, 8, msr))[0]
        finally:
            os.close(fd)
    except OSError:
        return None


def cpuid(leaf, subleaf=0, cpu=0):
    try:
        fd = os.open(f"/dev/cpu/{cpu}/cpuid", os.O_RDONLY)
        try:
            return struct.unpack("<4I", os.pread(fd, 16, (subleaf << 32) | leaf))
        finally:
            os.close(fd)
    except OSError:
        return None


def efi_var_byte(name):
    data = read(next(iter(glob.glob(f"/sys/firmware/efi/efivars/{name}-*")), ""), binary=True)
    return data[-1] if data else None


# ------------------------------------------------------------------ report --

def main():
    if os.geteuid() != 0:
        sys.exit("run as root: sudo python3 nuc_probe.py")
    run("modprobe", "msr")
    run("modprobe", "cpuid")

    lines, warn, rec = [], [], []
    p = lines.append
    dmesg = run("dmesg")
    cpu = parse_cpuinfo(read("/proc/cpuinfo") or "")

    p("== system")
    dmi = lambda k: (read(f"/sys/class/dmi/id/{k}") or "?").strip()
    p(f"product: {dmi('sys_vendor')} {dmi('product_name')} / board {dmi('board_name')}")
    p(f"firmware: {dmi('bios_vendor')} {dmi('bios_version')} ({dmi('bios_date')})")
    uefi = os.path.isdir("/sys/firmware/efi")
    sb = efi_var_byte("SecureBoot") if uefi else None
    p(f"boot mode: {'UEFI' if uefi else 'legacy BIOS'}; Secure Boot: "
      f"{'n/a' if sb is None else ('ON' if sb else 'off')}")
    if sb:
        warn.append("Secure Boot is on: the GRUB image is unsigned; disable it in firmware setup.")
    p(f"kernel: {os.uname().release}")

    p("\n== cpu")
    p(f"{cpu['name']} ({cpu['vendor']}) family {cpu['family']:#x} model {cpu['model']:#x} "
      f"stepping {cpu['stepping']}, {cpu['cpus']} logical CPUs")
    want = ["constant_tsc", "nonstop_tsc", "tsc_known_freq", "tsc_deadline_timer",
            "x2apic", "pdpe1gb", "fsgsbase", "pcid", "invpcid", "xsave", "hypervisor"]
    p("flags: " + " ".join(f"{f}={'y' if f in cpu['flags'] else 'n'}" for f in want))
    if "hypervisor" in cpu["flags"]:
        warn.append("Running under a hypervisor: TSC/firmware numbers are not bare metal.")
    if not {"constant_tsc", "nonstop_tsc"} <= cpu["flags"]:
        warn.append("TSC is not invariant: BOOTPROF microsecond values are unreliable.")

    p("\n== TSC frequency")
    path = sel4_tsc_path(cpu)
    p(f"seL4 tsc_init path: {'MSR_PLATFORM_INFO' if path == 'msr' else 'PIT calibration (~100 ms)'}")
    pinfo = rdmsr(MSR_PLATFORM_INFO)
    pinfo_mhz = platform_info_mhz(pinfo, cpu["model"]) if pinfo is not None else None
    p(f"MSR_PLATFORM_INFO: {'unreadable' if pinfo is None else hex(pinfo)} -> {pinfo_mhz} MHz")
    c15, c16 = cpuid(0x15), cpuid(0x16)
    c15_hz = cpuid15_tsc_hz(*c15[:3], cpu["model"]) if c15 else None
    p(f"CPUID.15H: {c15 and tuple(hex(v) for v in c15[:3])} -> "
      f"{c15_hz / 1e6 if c15_hz else None} MHz")
    p(f"CPUID.16H base: {c16[0] if c16 else None} MHz")
    linux_mhz = parse_dmesg_tsc_mhz(dmesg)
    p(f"Linux TSC calibration: {linux_mhz} MHz")
    best_hz = (round(linux_mhz * 1e6) if linux_mhz else None) or c15_hz or (pinfo_mhz and pinfo_mhz * 10**6)
    if best_hz:
        rec.append(f"scripts/build-baremetal.sh --tsc-hz {best_hz}")
    if path == "pit":
        rec.append("seL4 will spend ~100 ms in PIT calibration unless --tsc-hz is set.")

    p("\n== time before the Linux kernel (validates BOOTPROF entry_us)")
    adj = rdmsr(MSR_TSC_ADJUST) if "tsc_adjust" in cpu["flags"] else None
    p(f"IA32_TSC_ADJUST: {adj if adj is not None else 'n/a'}")
    if adj:
        warn.append("IA32_TSC_ADJUST != 0: firmware moved the TSC; entry_us is not time since reset.")
    tsc, uptime = rdmsr(MSR_TSC), read("/proc/uptime")
    if tsc is not None and uptime and best_hz:
        pre = tsc / best_hz - float(uptime.split()[0])
        p(f"TSC/f - uptime = {pre:.3f} s  (firmware + boot loader, if TSC counts from reset)")
    fpdt = "/sys/firmware/acpi/fpdt/boot"
    if os.path.isdir(fpdt):
        for f in sorted(os.listdir(fpdt)):
            p(f"FPDT {f}: {(read(f'{fpdt}/{f}') or '').strip()}")
    sa = run("systemd-analyze", "time").strip().splitlines()
    if sa:
        p(f"systemd-analyze: {sa[0]}")

    p("\n== display")
    fb = parse_efifb(dmesg)
    if not fb:  # simpledrm (Ubuntu 24.04+): address from iomem, mode from fb0
        g = lambda k: (read(f"/sys/class/graphics/fb0/{k}") or "").strip()
        try:
            fb = parse_fb0(g("name"), g("virtual_size"), g("bits_per_pixel"), g("stride"))
        except ValueError:
            fb = None
        addr = parse_iomem_bootfb(read("/proc/iomem") or "")
        if fb is not None and addr is not None:
            fb["addr"] = addr
    p(f"boot framebuffer: {fb and {k: (hex(v) if k == 'addr' else v) for k, v in fb.items()}}")
    if fb and {"addr", "width", "height", "pitch"} <= fb.keys():
        scale, cols, rows = console_geometry(fb["width"], fb["height"])
        p(f"BOOTPROF console: {scale}x font -> {cols}x{rows} cells")
        if fb["addr"] + fb["pitch"] * fb["height"] > PADDR_TOP:
            warn.append(f"framebuffer {fb['addr']:#x} is beyond seL4's kernel window; no screen output.")
        if fb.get("bpp") not in (24, 32):
            warn.append(f"framebuffer bpp={fb.get('bpp')}: the console only draws 24/32 bpp.")
        mt = mtrr_type_at(read("/proc/mtrr") or "", fb["addr"])
        p(f"MTRR type at framebuffer: {mt or 'default'}")
    elif uefi:
        warn.append("boot framebuffer not found (GPU driver took over); reboot the live USB "
                    "with 'nomodeset' on the kernel command line and rerun.")
    for l in run("lspci", "-nn").splitlines():
        if re.search(r"VGA|3D|Display", l):
            p(f"gpu: {l}")

    p("\n== serial / ACPI / memory")
    uarts = parse_serial(read("/proc/tty/driver/serial") or "")
    p(f"UARTs: {({k: (u, hex(port)) for k, (u, port) in uarts.items()}) or 'none detected'}")
    if any(port == 0x3F8 for _, port in uarts.values()):
        p("COM1 (0x3F8) present: seL4's serial output will appear there.")
    tables = sorted(os.listdir("/sys/firmware/acpi/tables")) if os.path.isdir("/sys/firmware/acpi/tables") else []
    p(f"ACPI tables: {' '.join(t for t in tables if t.isupper() or t[:4].isupper())}")
    if "DMAR" in tables:
        p("DMAR present (VT-d); the image builds with KernelIOMMU=OFF.")
    e820 = parse_e820(dmesg)
    ok = region_usable(e820, *SEL4_LOAD_REGION)
    p(f"e820 usable over [{SEL4_LOAD_REGION[0]:#x}, {SEL4_LOAD_REGION[1]:#x}): {ok}")
    if e820 and not ok:
        warn.append("low memory where seL4 loads its kernel/root task is not all usable RAM.")
    for s, e, t in e820:
        if s < SEL4_LOAD_REGION[1]:
            p(f"  {s:#011x}-{e:#011x} {t}")

    p("\n== warnings")
    lines += [f"! {w}" for w in warn] or ["(none)"]
    p("\n== recommendations")
    lines += [f"- {r}" for r in rec] or ["(none)"]

    report = "\n".join(lines) + "\n"
    sys.stdout.write(report)
    with open("sel4-probe.txt", "w") as f:
        f.write(report)
    print("\nwrote ./sel4-probe.txt")


if __name__ == "__main__":
    main()
