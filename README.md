# Caligula Burning Tool

[![CI](https://github.com/ifd3f/caligula/actions/workflows/ci.yml/badge.svg)](https://github.com/ifd3f/caligula/actions/workflows/ci.yml)

![Screenshot of the Caligula TUI verifying a disk.](./images/verifying.png)

_Caligula_ is a user-friendly, lightweight TUI for imaging disks.

```
$ caligula burn -h
A lightweight, user-friendly disk imaging tool

Usage: caligula burn [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Input file to burn

Options:
  -o <OUT>                         Where to write the output. If not supplied, we will search for possible disks and ask you for where you want to burn
  -z, --compression <COMPRESSION>  What compression format the input file is in [default: ask] [possible values: ask, auto, none, gz, bz2, xz]
  -s, --hash <HASH>                The hash of the input file. For more information, see long help (--help) [default: ask]
      --hash-of <HASH_OF>          Is the hash calculated from the raw file, or the compressed file? [possible values: raw, compressed]
      --show-all-disks             If provided, we will show all disks, removable or not
      --interactive <INTERACTIVE>  If we should run in interactive mode or not [default: auto] [possible values: auto, always, never]
  -f, --force                      If supplied, we will not ask for confirmation before destroying your disk
      --root <ROOT>                If we don't have permissions on the output file, should we try to become root? [default: ask] [possible values: ask, always, never]
  -h, --help                       Print help (see more with '--help')
  -V, --version                    Print version
```

## How to install

- Arch Linux: [download it from the AUR](https://aur.archlinux.org/packages/caligula-bin)
- Nix Package Manager: If your system is flake-enabled, `nix run github:ifd3f/caligula`
- MacOS and other Linux distros: download the [latest release](https://github.com/ifd3f/caligula/releases/latest)

### Platform support matrix

| Platform         | Automated tests | Automated builds | Published binaries |
|------------------|-----------------|------------------|--------------------|
| `x86_64-linux`   | ✅               | ✅                | ✅                  |
| `x86_64-darwin`  | ✅               | ✅                | ✅                  |
| `aarch64-linux`  | ❌               | ✅                | ✅                  |
| `aarch64-darwin` | ❌               | ❌                | ✅*                 |

*This is not automated right now. We manually publish this once we remember to build it.

Linux for other architectures theoretically works, but we are not making any guarantees.

We plan on supporting Windows, FreeBSD, and OpenBSD eventually. If you would like support for other OSes and architectures, please file an issue!

## Features

- Small, statically-linked binary on the Linux version
- Cool graphs
- Listing attached disks, and telling you their size and hardware model information
- Rich confirmation dialogs so you don't accidentally nuke your filesystem
- Automatically decompressing your input file for a variety of formats, including gz, bz2, and xz
- Validating your input file against a hash before burning, with support for md5, sha1, sha256, and more!
- Running sudo/doas/su for you if you forgot to run in `root` earlier (it happens)
- Verifying your disk to make sure it was written correctly
- Did I mention cool graphs?

## FAQ

### Why did you make this?

Because I wanted a nice, user-friendly wrapper around `dd` that wasn't like, a 90 MB executable that packages Chromium and eats hundreds of MB of RAM like certain other disk etching softwares do.

### Why is it called "Caligula"?

Because there used to be a tool called Nero Burning ROM, so I chose another crazy Roman emperor to name this software after. It's a very uncreative name and I might rename it later.

### Why is `dd` not good enough for you?

I know how `dd` works. In fact, instead of using `caligula`, I could just do this:

```
$ sha256sum some-image-file.iso.gz
```
> I pause here to confirm that the file has the right SHA.
```
$ gunzip some-image-file.iso.gz
$ lsblk
```
> I pause here to make sure my disk is indeed detected by the OS.
```
$ dd bs=4M if=some-image-file.iso of=/dev/
```
> I pause here to confirm that I am indeed typing in the correct disk.
```
$ dd bs=4M if=some-image-file.iso of=/dev/sdb
dd: failed to open '/dev/sdb': Permission denied
$ sudo dd bs=4M if=some-image-file.iso of=/dev/sdb
```
> There is no output, but I'd like to see the progress.
```
^C^C^C
$ sudo dd bs=4M if=some-image-file.iso of=/dev/sdb status=progress
```

Or, instead of that whole song and dance, I could just type

```
$ caligula burn some-image-file.iso.gz
```

and have it fill in the blanks. It's not that I don't know how to use `dd`, it's just that after flashing so many SD cards and USBs, I'd rather do something less error-prone.

### Why Rust?

Because it's 🚀🚀🚀 BLAZING FAST 🚀🚀🚀 and 💾💾💾 MEMORY SAFE 💾💾💾

### Why Nix?

It makes the CI more predictable.

### Why so many other dependencies?

To be fair, Rust doesn't have a very comprehensive standard library, and I only use one or two functions in most of those dependencies. Thanks to dead code elimination, inlining, and other optimizations, they don't end up contributing much to the overall binary size.

### Will the binary ever get bigger?

I want to keep the binary very small. I want to keep the x86_64-linux version under 4MB, with 8MB as an absolute maximum. As of v0.3.0, it's only 2.66MB, which is pretty good!
