#!/usr/bin/env python3

"""
Regenerates the part of README.md that contains the --help output.
"""

import subprocess
import sys

START_MARKER = "<!-- BEGIN GENERATED HELP OUTPUT -->"
END_MARKER = "<!-- END GENERATED HELP OUTPUT -->"

try:
    caligula = sys.argv[1]
except IndexError:
    print("Provide path to caligula executable")
    sys.exit(1)


help_output = (
    subprocess.check_output(
        ["caligula", "-h"],
        executable=caligula,
        env={"_CALIGULA_CONFIGURE_CLAP_FOR_README": "1"},
    )
    .decode("utf8")
    .strip()
)

before = []
after = []

with open("README.md", "r") as f:
    # read until start marker
    for l in f:
        if START_MARKER in l:
            break
        before.append(l)

    # skip until end marker
    for l in f:
        if END_MARKER in l:
            break

    # read until end
    for l in f:
        after.append(l)


assert (
    len(after) > 0
), f"Did not find paired {START_MARKER} and {END_MARKER} tags in README"


with open("README.md", "w") as f:
    for l in before:
        f.write(l)

    f.write(START_MARKER)
    f.write("\n\n```\n")
    f.write("$ caligula\n")
    f.write(help_output)
    f.write("\n```\n\n")
    f.write(END_MARKER)
    f.write("\n")

    for l in after:
        f.write(l)
