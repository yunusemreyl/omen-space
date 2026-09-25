# Maintainer: Yunus Emre YILMAZ <yunusemreyl>

pkgname=omen-space-git
_pkgname=Omen-Space
pkgver=2.1.2
pkgrel=1
pkgdesc="Advanced HP Omen/Victus laptop manager for Linux with RGB, Fan, and MUX control"
arch=('x86_64')
url="https://github.com/yunusemreyl/omen-space"
license=('GPL')
depends=('dkms' 'polkit' 'gtk4' 'libadwaita')
makedepends=('git' 'gcc' 'make' 'pkg-config' 'rust')
provides=('omen-space')
conflicts=('omen-space' 'hp-laptop-manager' 'omenctl')
source=('git+https://github.com/yunusemreyl/omen-space.git')
sha256sums=('SKIP')

pkgver() {
  cd "$srcdir/${pkgname%-git}"
  git describe --long --tags | sed 's/\([^-]*-\)g/r\1/;s/-/./g' | sed 's/^v//'
}

build() {
  cd "$srcdir/${pkgname%-git}"
  cargo build --release --locked
}

package() {
  cd "$srcdir/${pkgname%-git}"

  # Install directories
  mkdir -p "$pkgdir/usr/libexec/omen-space"
  mkdir -p "$pkgdir/etc/omen-space"
  mkdir -p "$pkgdir/etc/dbus-1/system.d"
  mkdir -p "$pkgdir/usr/lib/systemd/system"
  mkdir -p "$pkgdir/usr/lib/sysusers.d"
  mkdir -p "$pkgdir/usr/lib/udev/rules.d"
  mkdir -p "$pkgdir/usr/bin"
  mkdir -p "$pkgdir/usr/share/applications"
  mkdir -p "$pkgdir/usr/share/dbus-1/services"
  mkdir -p "$pkgdir/usr/share/pixmaps"
  mkdir -p "$pkgdir/usr/share/icons/hicolor/512x512/apps"
  mkdir -p "$pkgdir/usr/share/omen-space/assets"
  mkdir -p "$pkgdir/etc/xdg/autostart"

  # Binaries
  cp target/release/omen-space-daemon "$pkgdir/usr/libexec/omen-space/"
  cp target/release/omen-cli "$pkgdir/usr/bin/"
  cp target/release/omen-tray "$pkgdir/usr/bin/"
  cp target/release/omen-gui "$pkgdir/usr/bin/"

  # System configuration files
  cp data/org.hp.omen.conf "$pkgdir/etc/dbus-1/system.d/"
  cp data/omen-space-daemon.service "$pkgdir/usr/lib/systemd/system/"
  cp data/sysusers.d/omen-space.conf "$pkgdir/usr/lib/sysusers.d/"
  cp data/99-omen-space.rules "$pkgdir/usr/lib/udev/rules.d/"

  # Desktop integration and assets
  cp data/org.hp.OmenSpace.desktop "$pkgdir/usr/share/applications/"
  cp data/org.hp.OmenSpace.service "$pkgdir/usr/share/dbus-1/services/"
  cp src/omen-gui/assets/omenspace.png "$pkgdir/usr/share/pixmaps/omenspace.png"
  cp src/omen-gui/assets/omenspace.png "$pkgdir/usr/share/icons/hicolor/512x512/apps/omenspace.png"
  cp -r src/omen-gui/assets/* "$pkgdir/usr/share/omen-space/assets/"

  # Autostart tray
  cat <<EOF > "$pkgdir/etc/xdg/autostart/omenspace-tray.desktop"
[Desktop Entry]
Name=OMENSpace Tray
Comment=OMENSpace System Tray Icon
Exec=/usr/bin/omen-tray
Icon=omenspace
Terminal=false
Type=Application
Categories=Utility;
EOF

  # DKMS Driver
  _dkms_dir="$pkgdir/usr/src/hp-omen-extra-${pkgver}"
  mkdir -p "$_dkms_dir"
  cp driver/hp-wmi.c "$_dkms_dir/"
  cp driver/hp-omen-extra.c "$_dkms_dir/"
  cp driver/Makefile "$_dkms_dir/"
  cp driver/dkms.conf "$_dkms_dir/"

  # Set version in dkms.conf
  sed -i "s/PACKAGE_VERSION=.*/PACKAGE_VERSION=\"${pkgver}\"/" "$_dkms_dir/dkms.conf"
}
