#!/usr/bin/env bash

branchname="update/$(date --rfc-3339=date)"
echo "branch name: $branchname"

set -euxo pipefail

git checkout -b "$branchname"
nix flake update
git commit -a -m "nix flake update"

nix develop --command cargo update
git commit -a -m "cargo update" 

