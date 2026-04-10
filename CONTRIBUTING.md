# How to contribute

Thank you for your interest in contributing!

Development of Caligula is done around the [Github repository](https://github.com/ifd3f/caligula). When contributing to the repository, if the change is relatively trivial, feel free to submit a pull request directly without opening an issue. However, for anything that may require larger changes, please first discuss the change you wish to make by [creating an issue](https://github.com/ifd3f/caligula/issues/new/choose).

## Developer environment

Installing [Nix](https://nixos.org/) is generally recommended, but technically not necessary. It's used to provide various development niceties, such as:
- **A developer shell!** It has cross-compilation support for all cross-compilation targets supported on a given system. You can run `nix develop` or use [direnv](https://direnv.net/) to get your shell.
- **A developer VM!** It can be used for emulating other architectures, and to simulate attaching and removing USB drives! See [its README](./nix/devvm/README.md) for more info.
- **Continuous integration test running!** Provided through [NixOS VM tests](https://wiki.nixos.org/wiki/NixOS_VM_tests).
- **CI reproduction!** The CI invokes Nix as well, and we treat it as the single source of truth for what works and what doesn't.

However, if you can't install, or don't want to install Nix, this is a relatively standard Cargo project so you can use the standard Rust tooling to edit it.

To perform linting checks locally, you can run `scripts/lint.sh` or `nix run .#lint-script`.

## Pull request process

Once you've made your changes and have submitted your PR, **please try to ensure all checks pass!** However, if it makes sense to merge something before all checks are green (for example, due to known CI failures), that's fine.

You may merge the PR once you have the sign-off of a maintainer (most likely the the Malevolent Dictator for Life [@ifd3f](https://github.com/ifd3f)), or if you do not have permission to do that, you may request the reviewer to merge it for you.

Other suggestions:
- Update the README.md with details of changes to the user-facing interface. This includes new (non-private) environment variables, useful file locations, CLI flags, and new behaviors.
- Feel free to refactor code, within reason. This project was hacked together in a week, and then several features were hacked on over the following month, and more features were slowly added over the next year, so there are lots of parts of the code that could use some de-crufting.

## Branching and release methodology

We currently use `main` as the primary development branch.

Anything merged into `main` should _ideally_ pass all checks and have a green CI. What's most critical is that things work when a release is made.

Releases are done periodically (though not necessarily with every PR).

### Squashes or merges?

Caligula uses a combination of both.
- Small changes (<200 LoC-ish or less) generally get squashed.
- I prefer to merge larger changes because it's good to track the development history of those changes.
