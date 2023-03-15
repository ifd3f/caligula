#!/bin/sh

set -euxo pipefail

chown -R user /caligula-bin

{
    cd /caligula-bin
    cp /inputs/caligula-bin-PKGBUILD ./PKGBUILD
    sudo -u user makepkg --printsrcinfo | tee .SRCINFO
}
