pkgname=caligula-bin
pkgver=0.3.0
pkgrel=1
pkgdesc="A lightweight, user-friendly disk imaging tool"
url="https://github.com/ifd3f/caligula"
license=("GPL-3.0")
arch=("x86_64")
provides=("caligula")
conflicts=("caligula")
source=("https://github.com/ifd3f/caligula/releases/download/v$pkgver/caligula-$CARCH-linux")
sha256sums=("ae1dda2649d7c9152b032b8ded1623bef8705296ea11d6060471ff3f63aa1046")

package() {
    mv caligula-x86_64-linux caligula
    install -Dm755 caligula -t "$pkgdir/usr/bin"
}
