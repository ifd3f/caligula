# pkgver, pkgrel, and sha256sums are inserted by script.

pkgname=caligula-bin
pkgdesc="A lightweight, user-friendly disk imaging tool"
url="https://github.com/ifd3f/caligula"
license=("GPL-3.0")
arch=("x86_64")
provides=("caligula")
conflicts=("caligula")
source=("https://github.com/ifd3f/caligula/releases/download/v$pkgver/caligula-$CARCH-linux")

package() {
    mv caligula-x86_64-linux caligula
    install -Dm755 caligula -t "$pkgdir/usr/bin"
}
