#!/usr/bin/env bash

repodir=$(dirname "$(dirname "$0")")

set -euxo pipefail

cd "$repodir"
nix flake update && nix build

