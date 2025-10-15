#!/usr/bin/env python3

import subprocess
import sys

START_MARKER = '<!-- BEGIN GENERATED HELP OUTPUT -->'
END_MARKER = '<!-- END GENERATED HELP OUTPUT -->'

caligula = sys.argv[1]


help_output = subprocess.check_output([caligula, 'burn', '-h']).decode('utf8').rstrip()

before = []
after = []

with open('README.md', 'r') as f:
    # read until start marker
    for l in f:
        if START_MARKER in f:
            break
        before.append(l)

    # skip until end marker
    for l in f:
        if END_MARKER in f:
            break

    # read until end
    for l in f:
        after.append(l)


with open('README.md', 'r') as f:
    for l in before:
        f.write(l)

    f.write(START_MARKER)
    f.write('\n```\n')
    f.write(help_output)
    f.write('\n```\n')
    f.write(END_MARKER)

    for l in after:
        f.write(l)
