#!/usr/bin/env python3

import os
import fcntl
import pty
import struct
import subprocess
import sys
import termios

START_MARKER = '<!-- BEGIN GENERATED HELP OUTPUT -->'
END_MARKER = '<!-- END GENERATED HELP OUTPUT -->'

try:
    caligula = sys.argv[1]
except IndexError:
    print("Provide path to caligula executable")
    sys.exit(1)


master_fd, slave_fd = pty.openpty()

fcntl.ioctl(master_fd, termios.TIOCSWINSZ, struct.pack('hhhh', 24, 80, 0, 0))

proc = subprocess.Popen(
    ['caligula', 'burn', '-h'],
    executable=caligula,
    stdin=slave_fd,
    stderr=slave_fd,
    stdout=slave_fd,
    env={'TERM': 'vt100'},
)
pty.close(slave_fd)

help_output = (
    os.read(master_fd, 1000000)
        .decode('utf8')
        .rstrip()
)
pty.close(master_fd)
proc.wait()

before = []
after = []

with open('README.md', 'r') as f:
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


with open('README.md', 'w') as f:
    for l in before:
        f.write(l)

    f.write(START_MARKER)
    f.write('\n```\n')
    f.write('$ caligula burn -h\n')
    f.write(help_output)
    f.write('\n```\n')
    f.write(END_MARKER)
    f.write('\n')

    for l in after:
        f.write(l)
