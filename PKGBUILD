# Builds the checkout this file sits in: makepkg -si
pkgname=codeburn-rs
pkgver=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$startdir/Cargo.toml" | head -n 1)
pkgrel=1
pkgdesc="Local AI coding spend from Claude, Codex, and Cursor session files"
arch=(x86_64 aarch64)
url="https://github.com/dvaxert/codeburn-rs"
license=(MIT)
depends=(curl gcc-libs glibc)
makedepends=(cargo)
# The build fails when makepkg turns on LTO.
options=(!lto)

prepare() {
  cd "$startdir"
  cargo fetch --locked
}

build() {
  cd "$startdir"
  CARGO_TARGET_DIR="$srcdir/target" cargo build --release --frozen
}

package() {
  cd "$startdir"
  install -Dm755 "$srcdir/target/release/codeburn-rs" "$pkgdir/usr/bin/codeburn-rs"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 THIRD-PARTY-NOTICES.md "$pkgdir/usr/share/licenses/$pkgname/THIRD-PARTY-NOTICES.md"
}
