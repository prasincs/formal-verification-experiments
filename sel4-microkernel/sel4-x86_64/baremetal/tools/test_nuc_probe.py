#!/usr/bin/env python3
"""Unit tests for nuc_probe.py parsers: python3 -I baremetal/tools/test_nuc_probe.py"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nuc_probe as np  # noqa: E402

CPUINFO_NUC9 = """processor\t: 0
vendor_id\t: GenuineIntel
cpu family\t: 6
model\t\t: 158
model name\t: Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz
stepping\t: 13
flags\t\t: fpu tsc constant_tsc nonstop_tsc tsc_adjust x2apic

processor\t: 1
vendor_id\t: GenuineIntel
"""

DMESG = """[    0.000000] BIOS-e820: [mem 0x0000000000000000-0x000000000009efff] usable
[    0.000000] BIOS-e820: [mem 0x000000000009f000-0x00000000000fffff] reserved
[    0.000000] BIOS-e820: [mem 0x0000000000100000-0x000000003fffffff] usable
[    0.000000] BIOS-e820: [mem 0x0000000040000000-0x00000000403fffff] reserved
[    0.000000] tsc: Detected 2400.000 MHz processor
[    0.512000] efifb: framebuffer at 0x4000000000, using 8100k, total 8100k
[    0.512001] efifb: mode is 1920x1080x32, linelength=7680, pages=1
[    2.100000] clocksource: tsc: mask: 0xffffffffffffffff
[    2.200000] tsc: Refined TSC clocksource calibration: 2399.998 MHz
"""


class Cpu(unittest.TestCase):
    def test_cpuinfo(self):
        c = np.parse_cpuinfo(CPUINFO_NUC9)
        self.assertEqual((c["family"], c["model"], c["stepping"]), (6, 0x9E, 13))
        self.assertIn("nonstop_tsc", c["flags"])
        self.assertEqual(c["cpus"], 2)

    def test_sel4_path(self):
        c = np.parse_cpuinfo(CPUINFO_NUC9)
        self.assertEqual(np.sel4_tsc_path(c), "pit")  # Coffee Lake not in seL4's list
        self.assertEqual(np.sel4_tsc_path({**c, "model": 0x5E}), "msr")
        self.assertEqual(np.sel4_tsc_path({**c, "model": 0x5E, "vendor": "AuthenticAMD"}), "pit")
        self.assertEqual(np.sel4_tsc_path({**c, "model": 0x5E, "family": 0xF}), "pit")

    def test_platform_info(self):
        self.assertEqual(np.platform_info_mhz(0x1800, 0x9E), 2400)
        self.assertEqual(np.platform_info_mhz(0x1500, 0x1E), 2799)  # Nehalem 133.33 MHz bus
        self.assertIsNone(np.platform_info_mhz(0, 0x9E))

    def test_apic_timer_hz(self):
        c = np.parse_cpuinfo(CPUINFO_NUC9)
        self.assertEqual(np.apic_timer_hz(c, (2, 200, 0, 0)), 24_000_000)        # table
        self.assertEqual(np.apic_timer_hz(c, (2, 200, 38_400_000, 0)), 38_400_000)  # ECX wins
        self.assertEqual(np.apic_timer_hz({**c, "flags": c["flags"] | {"hypervisor"}}, None), 1_000_000_000)
        self.assertIsNone(np.apic_timer_hz({**c, "model": 0x01}, (0, 0, 0, 0)))
        self.assertIsNone(np.apic_timer_hz({**c, "vendor": "AuthenticAMD"}, (2, 200, 0, 0)))

    def test_cpuid15(self):
        # Coffee Lake reports ECX=0: crystal comes from the model table.
        self.assertEqual(np.cpuid15_tsc_hz(2, 200, 0, 0x9E), 2_400_000_000)
        self.assertEqual(np.cpuid15_tsc_hz(2, 200, 25_000_000, 0x9E), 2_500_000_000)
        self.assertIsNone(np.cpuid15_tsc_hz(2, 200, 0, 0x01))  # unknown crystal
        self.assertIsNone(np.cpuid15_tsc_hz(0, 0, 0, 0x9E))


class Dmesg(unittest.TestCase):
    def test_tsc_prefers_refined(self):
        self.assertEqual(np.parse_dmesg_tsc_mhz(DMESG), 2399.998)
        self.assertEqual(np.parse_dmesg_tsc_mhz("tsc: Detected 2400.000 MHz processor"), 2400.0)
        self.assertIsNone(np.parse_dmesg_tsc_mhz(""))

    def test_efifb(self):
        self.assertEqual(np.parse_efifb(DMESG), {"addr": 0x4000000000, "width": 1920,
                                                 "height": 1080, "bpp": 32, "pitch": 7680})
        self.assertIsNone(np.parse_efifb(""))

    def test_e820_and_usable(self):
        r = np.parse_e820(DMESG)
        self.assertEqual(r[2], (0x100000, 0x40000000, "usable"))
        self.assertTrue(np.region_usable(r, 0x100000, 0x1000000))
        self.assertFalse(np.region_usable(r, 0x9F000, 0x200000))      # starts in reserved
        self.assertFalse(np.region_usable(r, 0x3FF00000, 0x40100000))  # runs into reserved
        self.assertFalse(np.region_usable([], 0x100000, 0x200000))

    def test_usable_across_adjacent_ranges(self):
        r = [(0x100000, 0x800000, "usable"), (0x800000, 0x2000000, "usable")]
        self.assertTrue(np.region_usable(r, 0x100000, 0x1000000))
        gap = [(0x100000, 0x800000, "usable"), (0x801000, 0x2000000, "usable")]
        self.assertFalse(np.region_usable(gap, 0x100000, 0x1000000))


class Simpledrm(unittest.TestCase):
    def test_iomem_bootfb(self):
        iomem = ("00100000-3fffffff : System RAM\n"
                 "4000000000-400fffffff : 0000:00:02.0\n"
                 "  4000000000-40007e8fff : BOOTFB\n")
        self.assertEqual(np.parse_iomem_bootfb(iomem), 0x4000000000)
        self.assertEqual(np.parse_iomem_bootfb("  80000000-807fffff : efifb\n"), 0x80000000)
        self.assertIsNone(np.parse_iomem_bootfb("00100000-3fffffff : System RAM\n"))

    def test_fb0(self):
        self.assertEqual(np.parse_fb0("simpledrmdrmfb", "1920,1080", "32", "7680"),
                         {"width": 1920, "height": 1080, "bpp": 32, "pitch": 7680})
        self.assertIsNotNone(np.parse_fb0("EFI VGA", "1280,800", "32", "5120"))
        self.assertIsNone(np.parse_fb0("i915drmfb", "1920,1080", "32", "7680"))
        self.assertIsNone(np.parse_fb0("", "", "", ""))


class Virtualization(unittest.TestCase):
    @staticmethod
    def allowed1(*bits):
        return sum(1 << (32 + b) for b in bits)

    def test_vmx_caps_coffee_lake_like(self):
        proc = self.allowed1(31)                       # secondary controls available
        proc2 = self.allowed1(1, 5, 7, 14, 17, 25)     # no APICv
        ept = (1 << 6) | (1 << 16) | (1 << 17) | (1 << 21) | (1 << 32)
        c = np.vmx_caps(0, proc, proc2, ept)
        self.assertTrue(c["ept"] and c["vpid"] and c["unrestricted_guest"] and c["pml"])
        self.assertFalse(c["apic_register_virt"] or c["virtual_intr_delivery"] or c["posted_interrupts"])
        self.assertTrue(c["ept_1g"] and c["ept_ad"] and c["invvpid"])

    def test_low_dword_ignored(self):
        # Allowed-0 settings (low dword) must not be read as capabilities.
        c = np.vmx_caps(1 << 7, self.allowed1(31), (1 << 1), 0)
        self.assertFalse(c["posted_interrupts"] or c["ept"])

    def test_no_secondary_controls(self):
        c = np.vmx_caps(0, 0, self.allowed1(1, 5), (1 << 17))
        self.assertFalse(c["ept"] or c["vpid"] or c["ept_1g"])  # also gates EPT caps

    def test_feature_control(self):
        self.assertEqual(np.feature_control_vmx(0b101), "enabled")
        self.assertEqual(np.feature_control_vmx(0b001), "disabled-locked")
        self.assertEqual(np.feature_control_vmx(0b000), "unlocked")

    def test_vtd_bits(self):
        self.assertEqual(np.decode_bits(1 << 59, np.VTD_CAP_BITS), {"posted_interrupts": True})
        self.assertEqual(np.decode_bits(1 << 3, np.VTD_ECAP_BITS),
                         {"interrupt_remapping": True, "pass_through": False})


class Misc(unittest.TestCase):
    def test_console_geometry_matches_kernel(self):
        self.assertEqual(np.console_geometry(1280, 800), (2, 80, 50))
        self.assertEqual(np.console_geometry(1920, 1080), (3, 80, 45))
        self.assertEqual(np.console_geometry(3840, 2160), (4, 120, 67))
        self.assertEqual(np.console_geometry(640, 480), (1, 80, 60))

    def test_mtrr(self):
        t = ("reg00: base=0x000000000 (    0MB), size= 2048MB, count=1: write-back\n"
             "reg01: base=0x080000000 ( 2048MB), size=  256MB, count=1: write-combining\n"
             "reg02: base=0x080000000 ( 2048MB), size=   16MB, count=1: uncachable\n")
        self.assertEqual(np.mtrr_type_at(t, 0x1000), "write-back")
        self.assertEqual(np.mtrr_type_at(t, 0x80000000), "uncachable")  # UC wins
        self.assertEqual(np.mtrr_type_at(t, 0x81000000), "write-combining")
        self.assertIsNone(np.mtrr_type_at(t, 0x4000000000))

    def test_serial(self):
        t = ("serinfo:1.0 driver revision:\n"
             "0: uart:16550A port:000003F8 irq:4 tx:0 rx:0\n"
             "1: uart:unknown port:000002F8 irq:3\n")
        self.assertEqual(np.parse_serial(t), {0: ("16550A", 0x3F8)})

    def test_paddr_top(self):
        self.assertEqual(np.PADDR_TOP, 0x7F80000000)


if __name__ == "__main__":
    unittest.main()
