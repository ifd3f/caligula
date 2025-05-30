#! /usr/bin/env python3
# Pexpect tests for the UI.
# Run this file from the repository root or specify the path to the Caligula binary
# in the environment variable `CALIGULA`.
# You can run this file with system Python.
# To install its dependencies on Debian/Ubuntu, execute the command:
# $ sudo apt install python3-pexpect

from __future__ import annotations

import filecmp
import os
import tempfile
import unittest
from pathlib import Path

import pexpect

CALIGULA = os.environ.get("CALIGULA", "target/debug/caligula")
TEST_DIR = Path(__file__).parent
TEST_HASH_FILE = TEST_DIR / "hash.txt"
TEST_ISO_FILE = TEST_DIR / "test.iso"


def spawn(*args: Path | str) -> pexpect.spawn:
    return pexpect.spawn(
        str(args[0]),
        [str(arg) for arg in args[1:]],
        timeout=5,
    )


class TestSimpleUI(unittest.TestCase):
    def test_cancel_immediately(self) -> None:
        child = spawn(CALIGULA, "burn", TEST_ISO_FILE)
        child.expect_exact("Is this okay?")
        child.sendline("n")
        child.expect("canceled")
        child.wait()

        self.assertEqual(child.exitstatus, 0)
        self.assertIsNone(child.signalstatus)

    def test_skip_hash(self) -> None:
        child = spawn(CALIGULA, "burn", TEST_ISO_FILE)
        child.expect_exact("Is this okay?")
        child.sendline("y")
        child.expect_exact("What is the file's hash")
        child.sendline("skip")
        child.expect_exact("Select target disk")
        child.sendeof()
        child.expect_exact("Operation was canceled by the user")
        child.wait()

        self.assertEqual(child.exitstatus, 0)
        self.assertIsNone(child.signalstatus)

    def test_hash_file_option(self) -> None:
        child = spawn(CALIGULA, "burn", TEST_ISO_FILE, "--hash-file", TEST_HASH_FILE)
        child.expect_exact("Is this okay?")
        child.sendline("y")
        child.expect_exact("Disk image verified successfully")
        child.sendintr()
        child.wait()

        self.assertIsNone(child.exitstatus)
        self.assertIsNotNone(child.signalstatus)

    def test_abort_by_default(self) -> None:
        with tempfile.NamedTemporaryFile() as f_temp:
            child = spawn(
                CALIGULA,
                "burn",
                TEST_ISO_FILE,
                "--hash-file",
                TEST_HASH_FILE,
                "-o",
                f_temp.name,
            )
            child.expect_exact("Is this okay?")
            child.sendline("y")
            child.expect_exact("Disk image verified successfully")
            child.expect_exact("THIS ACTION WILL DESTROY ALL DATA")
            child.sendline("")
            child.expect_exact("Aborting")
            child.wait()

            self.assertEqual(child.exitstatus, 0)
            self.assertIsNone(child.signalstatus)


class TestFancyUI(unittest.TestCase):
    def test_burn_ui_quits(self) -> None:
        with tempfile.NamedTemporaryFile() as f_temp:
            child = spawn(
                CALIGULA,
                "burn",
                TEST_ISO_FILE,
                "--hash-file",
                TEST_HASH_FILE,
                "-o",
                f_temp.name,
            )
            child.expect_exact("Is this okay?")
            child.sendline("y")
            child.expect_exact("THIS ACTION WILL DESTROY ALL DATA")
            child.sendline("y")
            child.expect_exact("Speed")
            child.expect_exact("Done!")
            child.sendline("q")
            child.wait()

            self.assertEqual(child.exitstatus, 0)
            self.assertIsNone(child.signalstatus)

            self.assertTrue(filecmp.cmp(TEST_ISO_FILE, f_temp.name, shallow=False))


if __name__ == "__main__":
    unittest.main()
