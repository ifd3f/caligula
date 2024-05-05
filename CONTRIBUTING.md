# How to contribute

Thank you for your interest in contributing!

Development of Caligula is done around the [Github repository](https://github.com/ifd3f/caligula). When contributing to the repository, if the change is relatively trivial, feel free to submit a pull request directly without opening an issue. However, for anything that may require larger changes, please first discuss the change you wish to make by [creating an issue](https://github.com/ifd3f/caligula/issues/new/choose).

## Developer environment

[Nix](https://nixos.org/) is used to provision developer shells. You can run `nix develop` or use [direnv](https://direnv.net/) to get your shell. The pre-made Nix shell provides lots of useful features, including cross-compilation.

However, if you can't install, or don't want to install Nix, this is a relatively standard Cargo project so you can use the standard Rust tooling to edit it.

To perform linting checks locally, you can run `scripts/lint.sh` or `nix run .#lint-script`.

## Pull request process

Once you've made your changes and have submitted your PR, **please ensure all checks pass!** The CI invokes the Nix build, and that is the single source of truth for our builds. Installing Nix is generally recommended, but technically not necessary.

You may merge the PR once you have the sign-off of a maintainer (most likely the the Malevolent Dictator for Life [@ifd3f](https://github.com/ifd3f)), or if you do not have permission to do that, you may request the reviewer to merge it for you.

Other suggestions:
- Update the README.md with details of changes to the user-facing interface. This includes new (non-private) environment variables, useful file locations, CLI flags, and new behaviors.
- Feel free to refactor code, within reason. This project was hacked together in a week, and then several features were hacked on over the following month, and more features were slowly added over the next year, so there are lots of parts of the code that could use some de-crufting.

## Branching and release methodology

We currently use `main` as the development branch. Changes should generally be merged into `main`, except for hotfixes (security vulnerabilities, glaring bugs, stuff like that), which will be handled separately.

Anything merged into `main` must pass all checks and have a green CI, but it does not necessarily have to be a releasable version of the code. Releases are done periodically (though not necessarily with every PR).

### Squashes or merges?

Caligula uses a combination of both.
- Small changes (<200 LoC-ish or less) generally get squashed.
- I prefer to merge larger changes because it's good to track the development history of those changes.
