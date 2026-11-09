#!/usr/bin/env python3
"""Unit tests for kvm_bench.py: python3 -I baremetal/tools/test_kvm_bench.py"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kvm_bench as kb  # noqa: E402

LOG = """Boot config: console_port = 0x3f8
BOOTPROF kernel tsc_mhz=2400 tsc_init_us=5
BOOTPROF kernel entry_us=31000 exit_us=33500 kernel_us=2500
BOOTPROF user tsc_mhz=2400 user_us=33900
seL4 x86_64 root server started
BOOTPROF done
""".splitlines()


class Parse(unittest.TestCase):
    def test_fields(self):
        r = kb.parse_bootprof(LOG)
        self.assertEqual(r["tsc_init_us"], 5)
        self.assertEqual(r["kernel_us"], 2500)
        self.assertEqual(r["user_minus_exit_us"], 400)
        self.assertEqual(r["tsc_mhz"], 2400)

    def test_ignores_non_bootprof_and_unknown_keys(self):
        r = kb.parse_bootprof(["foo entry_us=1", "BOOTPROF kernel bogus=7 exit_us=9"])
        self.assertEqual(r, {"exit_us": 9})

    def test_empty(self):
        self.assertEqual(kb.parse_bootprof([]), {})


class Stats(unittest.TestCase):
    def test_percentile_nearest_rank(self):
        v = list(range(1, 11))
        self.assertEqual(kb.percentile(v, 90), 9)
        self.assertEqual(kb.percentile(v, 100), 10)
        self.assertEqual(kb.percentile(v, 0), 1)
        self.assertEqual(kb.percentile([7], 90), 7)

    def test_summarize_skips_missing(self):
        rows = kb.summarize([{"a": 1}, {"a": 3}, {}], ("a", "b"))
        self.assertEqual(rows, [("a", 2, 1, 2.0, 3, 3)])


class Cmd(unittest.TestCase):
    def args(self, accel):
        return kb.argparse.Namespace(qemu="q", accel=accel, machine="pc", mem="256M",
                                     kernel="k.elf", rootserver="r.elf")

    def test_kvm_uses_host_cpu(self):
        c = kb.qemu_cmd(self.args("kvm"))
        self.assertEqual(c[c.index("-accel") + 1], "kvm")
        self.assertEqual(c[c.index("-cpu") + 1], "host")
        self.assertEqual(c[c.index("-kernel") + 1], "k.elf")
        self.assertEqual(c[c.index("-initrd") + 1], "r.elf")
        self.assertIn("-nodefaults", c)

    def test_tcg_cpu(self):
        c = kb.qemu_cmd(self.args("tcg"))
        self.assertTrue(c[c.index("-cpu") + 1].startswith("Nehalem"))


if __name__ == "__main__":
    unittest.main()
