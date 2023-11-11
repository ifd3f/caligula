#!/usr/bin/env python3

import argparse
import shutil
import subprocess
import logging

from pathlib import Path
from typing import Optional


tools_dir = Path(__file__).resolve().parent


def arg_parser():
    parser = argparse.ArgumentParser(
        description="Caligula AUR packaging script",
    )

    parser.add_argument(
        "out_dir",
        help="Output directory for the PKGBUILD files. Everything here will be clobbered!",
    )
    parser.add_argument(
        "--sha256sum", required=True, help="SHA256SUM of x86_64-linux executable"
    )
    parser.add_argument(
        "--pkgver",
        required=True,
        help="Version number of the package without the leading v.",
    )
    parser.add_argument(
        "--pkgrel",
        required=True,
        help="Sequential release number to distinguish between same builds of different versions. Usually set to 1.",
    )

    return parser


def main():
    args = arg_parser().parse_args()
    assert args.pkgver[0] != "v", "pkgver must not start with leading v"
    out_dir = Path(args.out_dir).resolve()
    logging.basicConfig(
        level=logging.DEBUG,
        format="----- %(asctime)s [%(levelname)s] %(message)s",
    )

    logging.info(f"Cleaning {out_dir}")
    out_dir.mkdir(parents=True, exist_ok=True)
    for c in out_dir.iterdir():
        shutil.rmtree(c)

    # TODO: make a non-bin PKGBUILD and run that too
    write_bin_pkgbuild(args, out_dir / "caligula-bin", "caligula-bin")
    run_makepkg(out_dir / "caligula-bin")
    test_caligula()


def write_bin_pkgbuild(args, out_dir: Path, target_name: str):
    template_path = tools_dir / f"{target_name}.PKGBUILD"
    target_path = out_dir / "PKGBUILD"

    logging.info(f"templating {template_path} into {target_path}")

    with template_path.open() as f:
        template = f.read()

    target_path.parent.mkdir(parents=True, exist_ok=True)
    with target_path.open("w") as f:
        f.write(f'sha256sums=("{args.sha256sum}")\n')
        f.write(f"pkgver={args.pkgver}\n")
        f.write(f"pkgrel={args.pkgrel}\n")
        f.write(template)


def run_makepkg(pkgbuild_dir: Path, makepkguser="user"):
    run_shell(f"chown -R {makepkguser} {pkgbuild_dir}")

    logging.info(f"generating .SRCINFO in {pkgbuild_dir}")
    run_shell(
        f"sudo -u {makepkguser} makepkg --printsrcinfo > .SRCINFO",
        cwd=pkgbuild_dir,
    )

    logging.info(f"executing makepkg --install")
    run_shell(
        f"yes | sudo -u {makepkguser} makepkg --install",
        cwd=pkgbuild_dir,
    )


def test_caligula():
    logging.info("Testing if we can run caligula")
    run_shell("caligula --version")


def run_shell(cmd: str, cwd: Optional[str] = None):
    logging.debug(f"Running shell command: {cmd}")
    subprocess.run(
        cmd,
        shell=True,
        cwd=cwd,
        check=True,
    )


if __name__ == "__main__":
    main()
