# Benchmarking

Caligula is meant to be a high-performance disk tool. Therefore, we have a standard(ish) testing methodology.

There is a benchmarking subsystem under the `caligula bench` subcommand that emulates various hot paths in the program.

`caligula bench` commands are all very dumb. They generally emit a progress bar and potentially some additional benchmarking data, and will happily destroy your disks if you tell them to. Actual timing is meant to be done by a different program.

Currently, [Hyperfine](https://github.com/sharkdp/hyperfine) is what we use. It's fully manual for the time being, but we do have plans to eventually introduce automated performance regression testing in CI, at least on pull requests.

To compare hashing performance between two different versions, for example, you can run a command like this:

```sh
hyperfine \
    -L impl ./caligula-0.5.0-rc.2,./caligula-iog \
    '{impl} bench hash 2023-05-03-raspios-buster-armhf-lite.img.xz -a sha256 -z xz'
```

For your convenience, I've added `/caligula` and `/caligula-*` into the .gitignore.
