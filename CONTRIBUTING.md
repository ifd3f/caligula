# How to contribute

Thank you for your interest in contributing!

Development of Caligula is done around the [Github repository](https://github.com/ifd3f/caligula). When contributing to the repository, please first discuss the change you wish to make by creating an issue. However, if the change is relatively trivial, feel free to submit a pull request directly without opening an issue.

## Pull request process

When opening a PR, you may open a PR as a draft, or not. Any PR that is not a draft will be considered ready for review.

Make sure to follow these general steps when making your code work:

1. Ensure all checks pass.
   - The Nix build is the single source of truth for builds. Installing Nix is generally recommended, but technically not necessary.
   - To perform linting checks locally, you can run `scripts/lint.sh` or `nix run .#lint-script`.
2. Update the README.md with details of changes to the interface. This includes new (non-private) environment variables, useful file locations, CLI flags, and new behaviors.
3. You may merge the PR once you have the sign-off of a maintainer (probably [@ifd3f](https://github.com/ifd3f)), or if you do not have permission to do that, you may request the reviewer to merge it for you.

Feel free to refactor code, within reason. This project was hacked together in a week, and then several features were hacked on over the following month, so there are lots of parts of the code that could use some de-crufting.

## Branching and release methodology

We currently use `main` as the development branch. Changes should generally be merged into `main`, except for hotfixes (security vulnerabilities, glaring bugs, stuff like that), which will be handled separately.

Anything merged into `main` must pass all checks, but it does not necessarily have to be a releasable version of the code. Before releases, which are done periodically (though not with every PR), some cleanup may be necessary.
