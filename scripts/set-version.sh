#!/usr/bin/env bash

if [[ -z ${1+x} ]] || [[ $1 == v* ]]; then
    echo "Usage: $0 [new version without leading v]"
    exit -1;
fi

version="$1"

dir="$(dirname "$(dirname "$0")")"
cargotoml="$dir/Cargo.toml"

sedcmd='0,/version =/s/version = .*/version = "'
sedcmd+="$1"
sedcmd+='"/'

set -euxo pipefail

sed -i "$sedcmd" "$cargotoml"

git add "$cargotoml"
git commit -m "bump version to v$version"

