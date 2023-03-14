# Caligula Burning Tool

[![CI](https://github.com/ifd3f/caligula/actions/workflows/ci.yml/badge.svg)](https://github.com/ifd3f/caligula/actions/workflows/ci.yml)

![Screenshot of the Caligula TUI verifying a disk.](./images/verifying.png)

_Caligula_ is a safe, user-friendly, lightweight TUI for imaging disks.

**WARNING!** This software is somewhat experimental. If you have problems, please file an issue and I will try to address it!

```
$ caligula burn -h
Burn an image to a disk

Usage: caligula burn [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Input file to burn

Options:
  -o <OUT>
          Where to write the output. If not supplied, we will search for
          possible disks and ask you for where you want to burn
  -z, --compression <COMPRESSION>
          What compression format the input file is. If `auto`, then we will
          guess based on the extension [default: auto] [possible values:
          auto, none, bz2, gz, xz]
  -s, --hash <HASH>
          The hash of the input file. For more information, see long help
          (--help) [default: ask]
      --hash-of <HASH_OF>
          Is the hash calculated from the raw file, or the compressed file? [possible
          values: raw, compressed]
      --show-all-disks
          If provided, we will show all disks, removable or not
  -f, --force
          If supplied, we will not ask for confirmation before destroying
          your disk
  -h, --help
          Print help (see more with '--help')
  -V, --version
          Print version
```

## Supported platforms

Currently, we officially support Linux and MacOS. However, Windows and FreeBSD support are planned.

ARM and x86 architectures are also officially supported.

## Features

- Minimal, statically linked binary of ~2MB on Linux
- Listing attached disks, and telling you their size and hardware model information
- Confirmation dialogs so you don't `dd` your filesystem
- Automatically decompressing your input file
- Verifying your disk to make sure it was written correctly
- Running sudo for you if you don't have permissions on a disk
- Cool graphs

## Planned features

- Support for more platforms
- Post-burn patching (i.e. adding `ssh` and `wpa_supplicant.conf` files to Raspberry Pi disks)
- Lightweight GUI

## FAQ

### Why did you make this?

Because I wanted a nice, user-friendly wrapper around `dd` that wasn't like, a 90 MB executable that packages Chromium and eats hundreds of MB of RAM like certain other disk burning softwares do.

### Why is it called "Caligula"?

Because there used to be a tool called Nero Burning ROM, so I chose another crazy Roman emperor to name this software after. It's a very uncreative name and I might rename it later.

### Why Rust?

Because it's 🚀🚀🚀 BLAZING FAST 🚀🚀🚀 and 💾💾💾 MEMORY SAFE 💾💾💾
