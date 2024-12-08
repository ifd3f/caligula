#!/usr/bin/env bash

set -euxo pipefail

cargo fmt --check
cargo clippy
