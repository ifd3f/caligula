# runs on x86_64-linux only
{ runCommand, caligula, pkgrel ? "1" }:
let
  sha256 = runCommand "caligula-sha256" { } ''
    sha256sum ${caligula}/bin/caligula | awk '{print $1}'
  '';
  pkgdesc = "A lightweight, user-friendly disk imaging TUI";

  pkgbuild = ''
    pkgname=caligula-bin
    pkgdesc="${pkgdesc}"
    pkgrel=${pkgrel}
    pkgver=${caligula.version}
    url="https://github.com/ifd3f/caligula"
    license=("GPL-3.0")
    arch=("x86_64")
    provides=("caligula")
    conflicts=("caligula")
    source=("https://github.com/ifd3f/caligula/releases/download/v$pkgver/caligula-$CARCH-linux")
    sha256sums=("%%SHA256SUM%%")

    package() {
        mv caligula-x86_64-linux caligula
        install -Dm755 caligula -t "$pkgdir/usr/bin"
    }
  '';

  srcinfo = ''
    pkgbase = caligula-bin
    	pkgdesc = ${pkgdesc}
    	pkgver = ${caligula.version}
    	pkgrel = ${pkgrel}
    	url = https://github.com/ifd3f/caligula
    	arch = x86_64
    	license = GPL-3.0
    	provides = caligula
    	conflicts = caligula
    	source = https://github.com/ifd3f/caligula/releases/download/v${caligula.version}/caligula-x86_64-linux
    	sha256sums = %%SHA256SUM%%

    pkgname = caligula-bin
  '';
in runCommand "caligula-bin-aur" { inherit srcinfo pkgbuild; } ''
  sha256=$(sha256sum ${caligula}/bin/caligula | awk '{print $1}')

  mkdir -p $out
  echo "$srcinfo" | sed "s/%%SHA256SUM%%/$sha256/" > $out/.SRCINFO
  echo "$pkgbuild" | sed "s/%%SHA256SUM%%/$sha256/" > $out/PKGBUILD
''
