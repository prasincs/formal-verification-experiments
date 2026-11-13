#!/usr/bin/env python3
"""Boot the seL4 BOOTPROF image as a QEMU/KVM guest N times and summarize.

    python3 kvm_bench.py --kernel K.elf --rootserver RS.elf [--runs 20]
        [--machine pc | microvm,acpi=on] [--accel kvm | tcg [--icount 0]]

--kernel kernel32.elf boots via multiboot; kernel-pvh.elf via PVH (required
for microvm). Without KVM, `--accel tcg --icount 0` makes guest time an
instruction count (1 insn = 1 ns): deterministic, comparable across
configurations, but blind to I/O latency and VM-exit cost.

Per run, it collects the guest's BOOTPROF values (guest TSC; under KVM the
guest TSC starts near 0 at VM creation, so entry_us is roughly guest firmware
plus loader time) and host_ms: wall time from spawning QEMU to the first
BOOTPROF line on the guest serial port. The kernel buffers its output until
the root task prints, so that line marks the root task running.

This is the baseline for the microVM work. Machine `pc` boots through SeaBIOS
and the multiboot option ROM; a PVH entry point (Firecracker, Cloud
Hypervisor, QEMU `microvm`) removes both.
"""
import argparse
import os
import re
import select
import statistics
import subprocess
import sys
import time

FIELDS = ("tsc_mhz", "tsc_init_us", "apic_khz", "apic_cal_us", "entry_us", "exit_us", "kernel_us", "user_us",
          "bytes", "early", "flush_us", "done_us")
KVM_CPU = "host"
# No "enforce": microvm requests features (spec-ctrl) that TCG lacks.
TCG_CPU = "Nehalem,-vme,+pdpe1gb,-xsave,-xsaveopt,-xsavec,-fsgsbase,-invpcid,+syscall,+lm"


def parse_bootprof(lines):
    """BOOTPROF lines -> {field: int}; adds user_minus_exit_us when possible."""
    out = {}
    for line in lines:
        if line.startswith("BOOTPROF "):
            for k, v in re.findall(r"(\w+)=(\d+)", line):
                if k in FIELDS:
                    out[k] = int(v)
    if "user_us" in out and "exit_us" in out:
        out["user_minus_exit_us"] = out["user_us"] - out["exit_us"]
    return out


def boot_path(lines):
    """'pvh' if the kernel took the PVH entry, else 'multiboot'. QEMU silently
    prefers multiboot when an image carries both headers (kernel32.elf)."""
    return "pvh" if any(l.startswith("PVH: start_info") for l in lines) else "multiboot"


def percentile(values, q):
    """Nearest-rank percentile (q in [0, 100]) of a non-empty list."""
    s = sorted(values)
    k = max(0, min(len(s) - 1, -(-q * len(s) // 100) - 1))
    return s[int(k)]


def summarize(runs, keys):
    rows = []
    for k in keys:
        vals = [r[k] for r in runs if k in r]
        if vals:
            rows.append((k, len(vals), min(vals), statistics.median(vals), percentile(vals, 90), max(vals)))
    return rows


def qemu_cmd(args):
    accel = ["-accel", "kvm", "-cpu", KVM_CPU] if args.accel == "kvm" else ["-accel", "tcg", "-cpu", TCG_CPU]
    if args.icount is not None:
        accel += ["-icount", f"shift={args.icount},sleep=off"]
    return [args.qemu, *accel, "-machine", args.machine, "-m", args.mem, "-smp", "1",
            "-nodefaults", "-no-reboot", "-display", "none", "-monitor", "none",
            "-serial", "stdio", "-kernel", args.kernel, "-initrd", args.rootserver]


def one_run(cmd, timeout):
    t0 = time.monotonic_ns()
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, stdin=subprocess.DEVNULL)
    buf, lines, first_ns = b"", [], None
    deadline = time.monotonic() + timeout
    try:
        while time.monotonic() < deadline:
            r, _, _ = select.select([proc.stdout], [], [], 0.05)
            if not r:
                if proc.poll() is not None:
                    break
                continue
            chunk = os.read(proc.stdout.fileno(), 65536)
            if not chunk:
                break
            buf += chunk
            *done, buf = buf.split(b"\n")
            for raw in done:
                line = raw.decode(errors="replace").rstrip("\r")
                lines.append(line)
                if first_ns is None and line.startswith("BOOTPROF"):
                    first_ns = time.monotonic_ns()
            if any(l == "BOOTPROF done" for l in lines):
                break
    finally:
        proc.kill()
        _, err = proc.communicate()
    res = parse_bootprof(lines)
    res["path"] = boot_path(lines)
    if first_ns is not None:
        res["host_ms"] = (first_ns - t0) / 1e6
    if "BOOTPROF done" not in lines:
        res["error"] = (err.decode(errors="replace").strip() or "\n".join(lines[-5:]) or "no output")[:400]
    return res


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--kernel", required=True, help="ELF32 seL4 kernel (build/.../kernel32.elf)")
    ap.add_argument("--rootserver", required=True, help="root task ELF (build/.../rootserver.elf)")
    ap.add_argument("--runs", type=int, default=10)
    ap.add_argument("--accel", choices=("kvm", "tcg"), default="kvm")
    ap.add_argument("--machine", default="pc")
    ap.add_argument("--mem", default="256M")
    ap.add_argument("--qemu", default="qemu-system-x86_64")
    ap.add_argument("--timeout", type=float, default=30.0)
    ap.add_argument("--expect-path", choices=("pvh", "multiboot"),
                    help="fail runs that took the other boot path")
    ap.add_argument("--icount", type=int, metavar="SHIFT",
                    help="TCG only: virtual time from instruction count (2^SHIFT ns per insn)")
    args = ap.parse_args(argv)
    if args.icount is not None and args.accel != "tcg":
        ap.error("--icount requires --accel tcg")
    if args.accel == "kvm" and not os.access("/dev/kvm", os.R_OK | os.W_OK):
        sys.exit("/dev/kvm not accessible (enable VT-x, load kvm_intel, or run with sudo)")

    cmd = qemu_cmd(args)
    print("cmd:", " ".join(cmd))
    runs, failures = [], 0
    for i in range(args.runs):
        r = one_run(cmd, args.timeout)
        if "error" not in r and args.expect_path and r["path"] != args.expect_path:
            r["error"] = f"booted via {r['path']}, expected {args.expect_path}"
        if "error" in r:
            failures += 1
            print(f"run {i}: FAILED: {r['error']}", file=sys.stderr)
            continue
        runs.append(r)
        print(f"run {i}: path={r['path']} host_ms={r.get('host_ms', float('nan')):.1f} "
              + " ".join(f"{k}={r[k]}" for k in ("tsc_init_us", "apic_cal_us", "entry_us", "kernel_us",
                                                  "user_minus_exit_us", "flush_us") if k in r))
    if not runs:
        sys.exit("all runs failed")
    paths = sorted({r["path"] for r in runs})
    print(f"\n{len(runs)} ok, {failures} failed; tsc_mhz={runs[0].get('tsc_mhz')}; boot path: {'/'.join(paths)}")
    if len(paths) > 1:
        print("WARNING: runs took different boot paths; numbers are not comparable")
    if any(r.get("early") for r in runs):
        print("WARNING: console buffer overflowed during boot (early=1): I/O is inside kernel_us")
    print(f"{'metric':20} {'n':>3} {'min':>10} {'median':>10} {'p90':>10} {'max':>10}")
    for k, n, lo, med, p90, hi in summarize(runs, ("host_ms", "entry_us", "tsc_init_us", "apic_cal_us",
                                                   "kernel_us", "user_minus_exit_us", "flush_us", "done_us")):
        print(f"{k:20} {n:>3} {lo:>10.1f} {med:>10.1f} {p90:>10.1f} {hi:>10.1f}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
