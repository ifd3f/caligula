# Caligula Burning Tool

[![CI](https://github.com/ifd3f/caligula/actions/workflows/ci.yml/badge.svg)](https://github.com/ifd3f/caligula/actions/workflows/ci.yml)

![Screenshot of the Caligula TUI verifying a disk.](./images/verifying.png)

**Caligula** is a safe, user-friendly, low-resource TUI for imaging disks.

**!!! Warning !!!** This software is new and experimental. If you have problems, please file an issue and I will try to address it!

```
$ caligula burn
Burn an image to a disk

Usage: caligula burn [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Input file to burn

Options:
  -o <OUT>              Where to write the output. If not supplied, we will search for possible disks and ask you for where you want to burn
  -f, --force           If supplied, we will not ask for confirmation before destroying your disk
      --show-all-disks  If provided, we will not only show you removable disks, but all disks. If you use this option, please proceed with caution!
  -h, --help            Print help
  -V, --version         Print version
```

## Features

- Listing attached disks, and telling you their size and hardware model information
- Confirmation dialogs so you don't `dd` your filesystem
- Running sudo for you if you don't have permissions on a disk
- Cool graphs

## Supported platforms

Currently, we only support Linux. However, MacOS, Windows, and BSD support are planned.

## Planned features

- Support for more platforms
- Support for compression formats
- Post-burn patching (i.e. adding `ssh` and `wpa_supplicant.conf` files to Raspberry Pi disks)
- Lightweight GUI

## FAQ

### Why did you make this?

Because I wanted a nice, user-friendly wrapper around `dd` that wasn't like, a 90 MB executable that packages Chromium and eats hundreds of MB of RAM like certain other disk burning softwares do.

### Why is it called "Caligula"?

Because there used to be a tool called Nero Burning ROM, so I chose another crazy Roman emperor to name this software after. It's a very uncreative name and I might rename it later.

### Why Rust?

Because it's 🚀🚀🚀 BLAZING FAST 🚀🚀🚀 and 💾💾💾 MEMORY SAFE 💾💾💾
