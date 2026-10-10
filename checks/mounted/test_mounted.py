#!/usr/bin/env python3
"""Linux integration tests using only freshly allocated, file-backed loop devices.

Run as root in a private mount namespace:
    sudo unshare --mount --propagation private env CALIGULA=/path/to/caligula \
        python3 checks/mounted/test_mounted.py
Requires pexpect, util-linux, and e2fsprogs.
"""

from __future__ import annotations

import hashlib
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import pexpect

CALIGULA = os.environ.get("CALIGULA", "target/debug/caligula")


def run(*args: str | Path, **kwargs) -> str:
    return subprocess.check_output([str(arg) for arg in args], text=True, **kwargs).strip()


@unittest.skipUnless(os.geteuid() == 0, "requires root and a private mount namespace")
class TestMountedTarget(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="caligula-mount-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.backing = self.root / "disk"
        with self.backing.open("wb") as disk:
            disk.truncate(32 * 1024 * 1024)
        subprocess.run(
            ["sfdisk", str(self.backing)],
            input="label: dos\nstart=2048, size=24576, type=83\nstart=26624, size=24576, type=83\n",
            text=True, check=True, stdout=subprocess.DEVNULL,
        )
        self.disk = run("losetup", "--find", "--show", "--partscan", self.backing)
        self.addCleanup(run, "losetup", "--detach", self.disk)
        self.partition = self.disk + "p1"
        self.mountpoint = self.root / "USB drive"
        self.mountpoint.mkdir()
        run("mkfs.ext4", "-q", "-F", self.partition)
        self.mount(self.partition, self.mountpoint)
        self.image = self.root / "input.iso"
        self.image.write_bytes(os.urandom(1024 * 1024))

    def mount(self, device: str, path: Path) -> None:
        run("mount", device, path)
        self.addCleanup(self.unmount_if_mounted, path)

    def unmount_if_mounted(self, path: Path) -> None:
        if os.path.ismount(path):
            run("umount", path)

    def spawn(self, target: str | None = None, *extra: str) -> pexpect.spawn:
        child = pexpect.spawn(
            CALIGULA,
            ["burn", str(self.image), "-o", target or self.disk,
             "--hash", "skip", "--compression", "none", "--interactive", "never", *extra],
            encoding="utf-8", timeout=20,
        )
        self.addCleanup(child.close, force=True)
        return child

    def finish(self, child: pexpect.spawn, success: bool = True) -> None:
        child.expect(pexpect.EOF)
        child.close()
        self.assertIsNone(child.signalstatus)
        if success:
            self.assertEqual(child.exitstatus, 0)
        else:
            self.assertNotEqual(child.exitstatus, 0)

    def digest(self) -> bytes:
        return hashlib.sha256(self.backing.read_bytes()).digest()

    def assert_image_matches(self, target: str) -> None:
        expected = self.image.read_bytes()
        with open(target, "rb") as disk:
            self.assertEqual(disk.read(len(expected)), expected)

    def test_decline_returns_to_picker_and_marks_mounted_target(self) -> None:
        child = self.spawn(None, "--show-all-disks")
        child.expect_exact("Unmount these filesystems before writing?")
        child.sendline("n")
        child.expect_exact("Select target disk")
        child.expect_exact("[mounted]")
        self.assertTrue(os.path.ismount(self.mountpoint))
        child.sendeof()
        self.finish(child)

    def test_cancel_final_confirmation_does_not_unmount(self) -> None:
        child = self.spawn()
        child.expect_exact("Unmount these filesystems before writing?")
        child.sendline("y")
        child.expect_exact("THIS ACTION WILL DESTROY ALL DATA")
        child.sendline("n")
        self.finish(child)
        self.assertTrue(os.path.ismount(self.mountpoint))

    def test_confirm_unmounts_partition_before_writing_whole_disk(self) -> None:
        child = self.spawn()
        child.expect_exact("Unmount these filesystems before writing?")
        child.sendline("y")
        child.expect_exact("THIS ACTION WILL DESTROY ALL DATA")
        child.sendline("y")
        child.expect_exact("Done!")
        self.finish(child)
        self.assertFalse(os.path.ismount(self.mountpoint))
        self.assert_image_matches(self.disk)

    def test_force_unmounts_partition_target_and_preserves_sibling_mount(self) -> None:
        sibling = self.root / "sibling"
        sibling.mkdir()
        run("mkfs.ext4", "-q", "-F", self.disk + "p2")
        self.mount(self.disk + "p2", sibling)
        child = self.spawn(self.partition, "--force")
        child.expect_exact("Done!")
        self.finish(child)
        self.assertFalse(os.path.ismount(self.mountpoint))
        self.assertTrue(os.path.ismount(sibling))
        self.assert_image_matches(self.partition)

    def test_busy_mount_prevents_writing(self) -> None:
        with (self.mountpoint / "busy").open("wb"):
            os.sync()
            before = self.digest()
            child = self.spawn(None, "--force")
            child.expect_exact("Failed to unmount disk")
            self.finish(child, success=False)
            self.assertTrue(os.path.ismount(self.mountpoint))
            self.assertEqual(self.digest(), before)

    def test_device_alias_is_not_treated_as_an_output_file(self) -> None:
        alias = self.root / "disk-alias"
        alias.symlink_to(self.disk)
        child = self.spawn(str(alias), "--force")
        child.expect_exact("Done!")
        self.finish(child)
        self.assertFalse(os.path.ismount(self.mountpoint))
        self.assert_image_matches(self.disk)

    def test_new_mount_after_confirmation_prevents_writing(self) -> None:
        child = self.spawn()
        child.expect_exact("Unmount these filesystems before writing?")
        child.sendline("y")
        child.expect_exact("THIS ACTION WILL DESTROY ALL DATA")
        extra = self.root / "new mount"
        extra.mkdir()
        run("mount", "--bind", self.mountpoint, extra)
        self.addCleanup(self.unmount_if_mounted, extra)
        os.sync()
        before = self.digest()
        child.sendline("y")
        child.expect_exact("was mounted after confirmation")
        self.finish(child, success=False)
        self.assertTrue(os.path.ismount(self.mountpoint))
        self.assertTrue(os.path.ismount(extra))
        self.assertEqual(self.digest(), before)

    def test_covered_target_does_not_unmount_unrelated_filesystem(self) -> None:
        run("mount", "-t", "tmpfs", "unrelated", self.mountpoint)
        self.addCleanup(self.unmount_if_mounted, self.mountpoint)
        marker = self.mountpoint / "unrelated"
        marker.write_text("keep mounted")
        os.sync()
        before = self.digest()
        child = self.spawn(None, "--force")
        child.expect_exact("covered by another mount")
        self.finish(child, success=False)
        self.assertEqual(marker.read_text(), "keep mounted")
        self.assertEqual(self.digest(), before)

    def test_replacement_at_same_path_requires_new_confirmation(self) -> None:
        child = self.spawn()
        child.expect_exact("Unmount these filesystems before writing?")
        child.sendline("y")
        child.expect_exact("THIS ACTION WILL DESTROY ALL DATA")
        # Keep the original mount alive at another path, so its ID cannot be reused.
        saved = self.root / "saved mount"
        saved.mkdir()
        run("mount", "--move", self.mountpoint, saved)
        self.addCleanup(self.unmount_if_mounted, saved)
        self.mount(self.partition, self.mountpoint)
        os.sync()
        before = self.digest()
        child.sendline("y")
        child.expect_exact("was mounted after confirmation")
        self.finish(child, success=False)
        self.assertTrue(os.path.ismount(self.mountpoint))
        self.assertEqual(self.digest(), before)

    def test_exclusive_open_rejects_mount_in_another_namespace(self) -> None:
        # Retain this mount only in a child namespace, where discovery cannot see it.
        holder = subprocess.Popen(
            ["unshare", "--mount", "--propagation", "private", sys.executable,
             "-c", "import sys; print('ready', flush=True); sys.stdin.read()"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
        )

        def release() -> None:
            holder.communicate(timeout=10)

        self.addCleanup(release)
        self.assertEqual(holder.stdout.readline().strip(), "ready")
        run("umount", self.mountpoint)
        self.assertFalse(os.path.ismount(self.mountpoint))
        os.sync()
        before = self.digest()
        child = self.spawn(None, "--force")
        child.expect_exact("Device is busy")
        self.finish(child, success=False)
        self.assertEqual(self.digest(), before)


if __name__ == "__main__":
    unittest.main(verbosity=2)
